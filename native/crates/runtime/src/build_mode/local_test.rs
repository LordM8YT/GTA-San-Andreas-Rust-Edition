//! Test › Server + players: the world as it is now (saved or not) on a
//! SARE server on this computer, with one to four player windows joined to
//! it. Their console lines show in Output; Stop closes them all. The real
//! world files are not touched: the server gets a copy in a test folder.
//! Players join through a [`Conditioner`], so Connection can make their
//! link slow or lossy while the test runs.
use super::BuildMode;
use anyhow::{Context, Result};
use sa_net::conditioner::{Conditioner, Link};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, Sender};

/// Most player windows one test opens.
pub const MAX_PLAYERS: usize = 4;

/// A running test: the server and the players' games.
pub struct LocalTest {
    children: Vec<(String, Child)>,
    lines: Receiver<(String, String)>,
    pub port: u16,
    /// Between the players and the server (None: it could not start, and
    /// players join the server directly).
    pub conditioner: Option<std::sync::Arc<Conditioner>>,
    /// Test › Server › Network.
    pub graphs: super::net_graphs::NetGraphs,
}
impl Drop for LocalTest {
    fn drop(&mut self) {
        // Kill the games before their connections close, so they do not
        // report a lost server.
        for (_, child) in &mut self.children {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
impl LocalTest {
    pub fn players(&self) -> usize {
        self.children.len().saturating_sub(1)
    }
}

/// Write the test folder: `content/<id>/` (the world as it is now),
/// `resources/` and `test.cfg` for the server. Returns the cfg's name.
pub fn prepare(
    project: &sa_editor::project::Project,
    doc: &sa_editor::history::Document,
    root: &Path,
    port: u16,
) -> Result<&'static str> {
    if root.exists() {
        std::fs::remove_dir_all(root).with_context(|| format!("clearing {}", root.display()))?;
    }
    let content = root
        .join(sa_scene::content_pack::DEFAULT_DIR)
        .join(&project.id);
    std::fs::create_dir_all(content.parent().unwrap())?;
    project
        .save_as(doc, &content, &project.id)
        .context("copying the world for the test server")?;
    let _ = std::fs::remove_file(content.join(sa_editor::project::AUTOSAVE));
    std::fs::create_dir_all(root.join("resources"))?;
    let cfg = format!(
        "# Written by SARE Studio for a local test; it is replaced each time.\n\
         sv_hostname \"SARE Studio test\"\n\
         set sv_master1 \"\"\n\
         sets content_pack {}\n\
         endpoint_add_tcp \"127.0.0.1:{port}\"\n",
        project.id
    );
    std::fs::write(root.join("test.cfg"), cfg)?;
    Ok("test.cfg")
}

/// A free port on this computer.
fn free_port() -> Result<u16> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    Ok(listener.local_addr()?.port())
}

/// Program `name` next to this one (`sa-server`, `sa-runtime`).
fn beside(name: &str) -> Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let path = exe
        .parent()
        .context("this program has no folder")?
        .join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
    anyhow::ensure!(path.is_file(), "{} is not next to SARE", path.display());
    Ok(path)
}

/// Pass every line `child` prints to `lines`, as `who`.
fn forward(child: &mut Child, who: &str, lines: &Sender<(String, String)>) {
    let pipes: Vec<Box<dyn std::io::Read + Send>> = [
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn std::io::Read + Send>),
    ]
    .into_iter()
    .flatten()
    .collect();
    for pipe in pipes {
        let lines = lines.clone();
        let who = who.to_owned();
        std::thread::spawn(move || {
            for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                if lines.send((who.clone(), line)).is_err() {
                    break;
                }
            }
        });
    }
}

impl BuildMode {
    /// Player windows still open in a running test (None: no test).
    pub fn local_test_players(&self) -> Option<usize> {
        self.local_test.as_ref().map(LocalTest::players)
    }
    /// Start a server with the world as it is now and `players` windows
    /// joined to it.
    pub fn start_local_test(&mut self, players: usize) {
        self.stop_local_test();
        self.commit_preview();
        match self.launch_local_test(players.clamp(1, MAX_PLAYERS)) {
            Ok(test) => {
                self.info(format!(
                    "Test: server on 127.0.0.1:{} with {} player window{}; their messages show here. Stop closes them.",
                    test.port,
                    test.players(),
                    if test.players() == 1 { "" } else { "s" }
                ));
                if test.conditioner.is_none() {
                    self.warn(
                        "Test: Connection is off for this test; players join the server directly.",
                    );
                } else if self.ui.test_link != Link::PERFECT {
                    self.info(format!(
                        "Test: players' connection is {}.",
                        describe(self.ui.test_link)
                    ));
                }
                self.local_test = Some(test);
            }
            Err(error) => self.error(format!("Could not start the test: {error:#}")),
        }
    }

    fn launch_local_test(&mut self, players: usize) -> Result<LocalTest> {
        let root = std::env::temp_dir().join(format!("sare-studio-test-{}", self.project.id));
        let port = free_port()?;
        let cfg = prepare(&self.project, &self.doc, &root, port)?;
        let (send, lines) = channel();
        let mut children = Vec::new();
        let mut server = Command::new(beside("sa-server")?)
            .args(["+exec", cfg])
            .current_dir(&root)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("starting sa-server")?;
        forward(&mut server, "Server", &send);
        children.push(("Server".to_string(), server));
        let server_address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
        let conditioner = Conditioner::start(server_address, self.ui.test_link)
            .ok()
            .map(std::sync::Arc::new);
        let graphs = super::net_graphs::NetGraphs::start(conditioner.as_ref());
        let join = conditioner.as_ref().map_or(server_address, |c| c.address());
        let content_dir = root.join(sa_scene::content_pack::DEFAULT_DIR);
        for n in 1..=players {
            let name = format!("Player {n}");
            let mut game = Command::new(std::env::current_exe()?)
                .arg("--play")
                .arg("--content")
                .arg(&self.project.id)
                .arg("--content-dir")
                .arg(&content_dir)
                .arg("--join")
                .arg(join.to_string())
                .arg("--name")
                .arg(&name)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .with_context(|| format!("starting {name}"))?;
            forward(&mut game, &name, &send);
            children.push((name, game));
        }
        Ok(LocalTest {
            children,
            lines,
            port,
            conditioner,
            graphs,
        })
    }

    /// Change the players' connection, also in a running test.
    pub fn set_test_link(&mut self, link: Link) {
        if link == self.ui.test_link {
            return;
        }
        self.ui.test_link = link;
        if let Some(conditioner) = self
            .local_test
            .as_ref()
            .and_then(|t| t.conditioner.as_ref())
        {
            conditioner.set_link(link);
            self.info(format!(
                "Test: players' connection is now {}.",
                describe(link)
            ));
        }
    }

    pub fn stop_local_test(&mut self) {
        if self.local_test.take().is_some() {
            self.info("Test stopped: the server and player windows are closed.");
        }
    }

    /// Once a frame: their lines into Output, and note who has closed.
    pub fn poll_local_test(&mut self) {
        let Some(test) = &mut self.local_test else {
            return;
        };
        let traffic = test
            .conditioner
            .as_ref()
            .map(|c| c.traffic())
            .unwrap_or_default();
        test.graphs.tick(&traffic, std::time::Instant::now());
        let mut lines = Vec::new();
        while let Ok(line) = test.lines.try_recv() {
            lines.push(line);
            if lines.len() > 200 {
                break;
            }
        }
        let mut closed = Vec::new();
        test.children
            .retain_mut(|(who, child)| match child.try_wait() {
                Ok(Some(status)) => {
                    closed.push((who.clone(), status.success()));
                    false
                }
                _ => true,
            });
        let server_gone = closed.iter().any(|(who, _)| who == "Server");
        for (who, line) in lines {
            let lower = line.to_ascii_lowercase();
            let text = format!("[{who}] {line}");
            if lower.contains("error") || lower.contains("panicked") {
                self.error(text);
            } else if lower.contains("warn") {
                self.warn(text);
            } else {
                self.info(text);
            }
        }
        for (who, ok) in closed {
            if ok {
                self.info(format!("[{who}] closed"));
            } else {
                self.warn(format!("[{who}] stopped with an error"));
            }
        }
        if server_gone {
            self.warn("The test server stopped; closing the player windows.");
            self.local_test = None;
        }
    }
}

/// "Mobile 4G (120 ms ping, ±40 ms, 2% loss, 500 KB/s)".
pub fn describe(link: Link) -> String {
    if link == Link::PERFECT {
        return "Perfect (no delay or loss)".into();
    }
    let mut parts = vec![format!("{} ms ping", link.ping_ms)];
    if link.jitter_ms > 0 {
        parts.push(format!("±{} ms", link.jitter_ms));
    }
    if link.loss > 0.0 {
        parts.push(format!("{:.0}% loss", link.loss * 100.0));
    }
    if link.bandwidth > 0 {
        parts.push(bandwidth_label(link.bandwidth));
    }
    format!(
        "{} ({})",
        link.preset().unwrap_or("Custom"),
        parts.join(", ")
    )
}

/// Bytes per second as "64 KB/s" or "1.0 MB/s".
pub fn bandwidth_label(bytes: u32) -> String {
    if bytes >= 1_000_000 {
        format!("{:.1} MB/s", bytes as f32 / 1_000_000.0)
    } else {
        format!("{} KB/s", bytes / 1000)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn connections_are_described_in_plain_words() {
        use sa_net::conditioner::Link;
        assert_eq!(super::describe(Link::PERFECT), "Perfect (no delay or loss)");
        assert_eq!(
            super::describe(Link::PRESETS[3].1),
            "Mobile 4G (120 ms ping, ±40 ms, 2% loss, 500 KB/s)"
        );
        let custom = Link {
            ping_ms: 50,
            ..Link::PERFECT
        };
        assert_eq!(super::describe(custom), "Custom (50 ms ping)");
    }

    #[test]
    fn the_test_folder_holds_the_world_as_it_is_now() {
        let base = std::env::temp_dir().join(format!("sare-local-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (project, mut doc) =
            sa_editor::project::Project::create(&base.join("world"), "test-world", "Test world")
                .unwrap();
        // Unsaved: renamed, and not written to the world's own files.
        doc.info.name = "Renamed".into();
        let root = base.join("server");
        let cfg = super::prepare(&project, &doc, &root, 30199).unwrap();
        let text = std::fs::read_to_string(root.join(cfg)).unwrap();
        assert!(text.contains("sets content_pack test-world"));
        assert!(text.contains("127.0.0.1:30199"));
        let manifest =
            sa_scene::content_pack::Manifest::read(&root.join("content/test-world")).unwrap();
        assert_eq!(
            (manifest.id.as_str(), manifest.name.as_str()),
            ("test-world", "Renamed")
        );
        assert!(root.join("resources").is_dir());
        let own = sa_scene::content_pack::Manifest::read(&base.join("world")).unwrap();
        assert_eq!(own.name, "Test world", "the real world is not touched");
        // Again: the old copy is replaced.
        super::prepare(&project, &doc, &root, 30199).unwrap();
        let _ = std::fs::remove_dir_all(&base);
    }
}
