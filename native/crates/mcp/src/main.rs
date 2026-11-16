//! `sa-mcp`: the SARE creator bridge. A [Model Context Protocol] server that
//! lets an AI assistant (Claude Desktop, Claude Code, Cursor, ...) help a
//! creator make worlds and scripts:
//!
//! - **Worlds** ([`worlds`]): list, create and edit Build Mode worlds
//!   (content packs with a `layout.json`), using the same code as Build Mode,
//!   so everything the assistant makes opens in the editor and plays.
//! - **Scripts** ([`scripts`]): list, read, write and create Lua resources in
//!   a server's `resources/` folder; Lua is syntax-checked before it is saved.
//! - **Guides** ([`guide`]): SARE's own docs and the list of Lua functions
//!   SARE really has, so the assistant does not have to guess.
//!
//! It speaks JSON-RPC over stdin/stdout (one message per line) and only
//! touches files inside the content and resources folders it was given.
//! Nothing here runs Lua, starts the game or reaches the network.
//! See docs/ai-creators.md.
//!
//! [Model Context Protocol]: https://modelcontextprotocol.io
mod guide;
mod protocol;
mod scripts;
mod worlds;

use anyhow::{bail, Result};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Folders the bridge may read and write.
#[derive(Clone, Debug, Default)]
pub struct Server {
    /// Content packs (Build Mode worlds), normally `<SARE>/content`.
    pub content: Option<PathBuf>,
    /// Server resources, normally `<server>/server-data/resources`.
    pub resources: Option<PathBuf>,
}

impl Server {
    pub fn content_dir(&self) -> Result<&Path> {
        match &self.content {
            Some(dir) => Ok(dir),
            None => bail!(
                "no content folder found; start sa-mcp with --content-dir \"<SARE folder>/content\""
            ),
        }
    }
    pub fn resources_dir(&self) -> Result<&Path> {
        match &self.resources {
            Some(dir) => Ok(dir),
            None => bail!(
                "no resources folder found; start sa-mcp with --resources-dir \"<server>/server-data/resources\""
            ),
        }
    }
}

const HELP: &str = "sa-mcp: SARE creator bridge (Model Context Protocol over stdio)

Usage: sa-mcp [--content-dir <dir>] [--resources-dir <dir>]

  --content-dir    content packs / Build Mode worlds (default: a `content`
                   folder next to sa-mcp or in the current folder or above)
  --resources-dir  server resources (default: `server-data/resources` next to
                   sa-mcp or in the current folder or above)

Add it to your AI app as an MCP server; see docs/ai-creators.md.";

fn main() -> ExitCode {
    let mut server = Server::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--content-dir" | "--resources-dir" => {
                let Some(value) = args.next() else {
                    eprintln!("{arg} needs a folder");
                    return ExitCode::from(2);
                };
                let dir = PathBuf::from(value);
                if !dir.is_dir() {
                    eprintln!("{} is not a folder", dir.display());
                    return ExitCode::from(2);
                }
                if arg == "--content-dir" {
                    server.content = Some(dir);
                } else {
                    server.resources = Some(dir);
                }
            }
            "--version" | "-V" => {
                println!("sa-mcp {}", env!("CARGO_PKG_VERSION"));
                return ExitCode::SUCCESS;
            }
            "--help" | "-h" => {
                println!("{HELP}");
                return ExitCode::SUCCESS;
            }
            other => {
                eprintln!("unknown argument {other:?}\n\n{HELP}");
                return ExitCode::from(2);
            }
        }
    }
    let starts = start_folders();
    server.content = server.content.or_else(|| find_up(&starts, "content"));
    server.resources = server
        .resources
        .or_else(|| find_up(&starts, "server-data/resources"));
    // stdout is the protocol; everything for people goes to stderr.
    eprintln!(
        "sa-mcp {}: content = {}, resources = {}",
        env!("CARGO_PKG_VERSION"),
        shown(&server.content),
        shown(&server.resources)
    );
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        if let Some(reply) = protocol::handle_line(&server, &line) {
            if writeln!(stdout, "{reply}")
                .and_then(|()| stdout.flush())
                .is_err()
            {
                break;
            }
        }
    }
    ExitCode::SUCCESS
}

fn shown(dir: &Option<PathBuf>) -> String {
    dir.as_ref()
        .map_or("not found".into(), |d| d.display().to_string())
}

/// The folder sa-mcp lives in and the current folder.
fn start_folders() -> Vec<PathBuf> {
    let mut starts = Vec::new();
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
    {
        starts.push(dir);
    }
    if let Ok(dir) = std::env::current_dir() {
        starts.push(dir);
    }
    starts
}

/// `relative` inside one of `starts` or up to four folders above it.
fn find_up(starts: &[PathBuf], relative: &str) -> Option<PathBuf> {
    starts.iter().find_map(|start| {
        start
            .ancestors()
            .take(5)
            .map(|dir| dir.join(relative))
            .find(|dir| dir.is_dir())
    })
}
