//! Publish: put a saved world on a SARE server, so players who join it
//! play this world. The server folder is the one with `server.cfg`; the
//! world goes to its `content/<id>/` (replacing an older copy) and
//! `server.cfg` names it with `sets content_pack <id>`.
use crate::project::Project;
use anyhow::{ensure, Context, Result};
use std::path::{Path, PathBuf};

/// What a publish did.
#[derive(Debug, PartialEq)]
pub struct Published {
    /// Where the world now is on the server.
    pub folder: PathBuf,
    /// `server.cfg` already named this world.
    pub was_named: bool,
}

/// Copy the project's saved files into `server` and name the world in its
/// `server.cfg`.
pub fn to_server(project: &Project, server: &Path) -> Result<Published> {
    let cfg_path = server.join("server.cfg");
    ensure!(
        cfg_path.is_file(),
        "{} is not a SARE server folder: it has no server.cfg (start sa-server there once to make one)",
        server.display()
    );
    let content = server.join(sa_scene::content_pack::DEFAULT_DIR);
    let folder = content.join(&project.id);
    ensure!(
        std::fs::canonicalize(&project.folder).ok() != std::fs::canonicalize(&folder).ok(),
        "the world is already in that server's content folder"
    );
    // Copy next to the old one first, then swap, so a failed copy leaves
    // the server's world as it was.
    let fresh = content.join(format!(".{}-publishing", project.id));
    if fresh.exists() {
        std::fs::remove_dir_all(&fresh)?;
    }
    crate::project::copy_tree(&project.folder, &fresh)
        .with_context(|| format!("copying the world to {}", content.display()))?;
    sa_scene::content_pack::Manifest::read(&fresh).context("the copied world does not open")?;
    if folder.exists() {
        std::fs::remove_dir_all(&folder)
            .with_context(|| format!("replacing {} (is the server running?)", folder.display()))?;
    }
    std::fs::rename(&fresh, &folder)?;
    let cfg = std::fs::read_to_string(&cfg_path)?;
    let (cfg, was_named) = name_world(&cfg, &project.id);
    if !was_named {
        std::fs::write(&cfg_path, cfg)?;
    }
    Ok(Published { folder, was_named })
}

/// `cfg` with its `content_pack` line set to `id` (added at the end if
/// it has none), and whether it named `id` already.
fn name_world(cfg: &str, id: &str) -> (String, bool) {
    let line = format!("sets content_pack {id}");
    let mut found = false;
    let mut already = false;
    let mut out: Vec<String> = Vec::new();
    for raw in cfg.lines() {
        let words: Vec<&str> = raw.split_whitespace().collect();
        let is_pack = words.len() >= 2
            && matches!(words[0], "set" | "sets" | "setr")
            && words[1] == "content_pack";
        if is_pack && !found {
            found = true;
            let value = words.get(2).map(|v| v.trim_matches('"'));
            already = value == Some(id);
            out.push(if already {
                raw.to_string()
            } else {
                line.clone()
            });
        } else if is_pack {
            // A second one would win: drop it.
            continue;
        } else {
            out.push(raw.to_string());
        }
    }
    if !found {
        out.push(String::new());
        out.push("# The world players join, published from SARE Studio.".into());
        out.push(line);
    }
    let mut text = out.join("\n");
    text.push('\n');
    (text, already)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cfg_names_the_world_once() {
        let (cfg, named) = name_world("sv_hostname \"Town\"\nensure chat\n", "my-town");
        assert!(!named);
        assert!(cfg.ends_with("sets content_pack my-town\n"));
        let (cfg, named) = name_world(
            "sets content_pack old\nensure chat\nset content_pack \"other\"\n",
            "my-town",
        );
        assert!(!named);
        assert_eq!(cfg, "sets content_pack my-town\nensure chat\n");
        let (_, named) = name_world("sets content_pack \"my-town\"\n", "my-town");
        assert!(named);
    }

    #[test]
    fn publishing_copies_the_world_and_replaces_an_older_copy() {
        let base = std::env::temp_dir().join(format!("sare-publish-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (project, _) = Project::create(&base.join("world"), "my-town", "My Town").unwrap();
        std::fs::write(project.folder.join(crate::project::AUTOSAVE), "{}").unwrap();
        let server = base.join("server");
        assert!(to_server(&project, &server).is_err(), "no server.cfg");
        std::fs::create_dir_all(server.join("content/my-town")).unwrap();
        std::fs::write(server.join("content/my-town/stale.txt"), "old").unwrap();
        std::fs::write(server.join("server.cfg"), "sv_hostname \"Town\"\n").unwrap();
        let published = to_server(&project, &server).unwrap();
        assert!(!published.was_named);
        assert!(published.folder.join("layout.json").is_file());
        assert!(
            !published.folder.join("stale.txt").exists(),
            "the old copy is replaced"
        );
        assert!(!published.folder.join(crate::project::AUTOSAVE).exists());
        let cfg = std::fs::read_to_string(server.join("server.cfg")).unwrap();
        assert!(cfg.contains("sets content_pack my-town"));
        assert!(to_server(&project, &server).unwrap().was_named);
        let _ = std::fs::remove_dir_all(&base);
    }
}
