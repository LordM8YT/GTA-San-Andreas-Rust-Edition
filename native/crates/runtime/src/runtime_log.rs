//! `sare-runtime.log`: what the game loaded, so a blank map or a missing
//! model can be explained from one file. It sits beside sa-runtime (in the
//! profile's `logs/` folder when that folder is read-only) and starts fresh
//! when it passes 1 MiB. Lines also go to stderr, which the launcher keeps.
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

/// Set by [`start`]: probes and tests print to stderr only.
static ACTIVE: AtomicBool = AtomicBool::new(false);

const FILE: &str = "sare-runtime.log";
const MAX_BYTES: u64 = 1024 * 1024;

/// Where the log is written.
pub fn path() -> PathBuf {
    static PATH: OnceLock<PathBuf> = OnceLock::new();
    PATH.get_or_init(|| {
        let beside = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(|dir| dir.join(FILE)));
        match beside {
            Some(path) if writable(&path) => path,
            _ => sa_client::config_dir().join("logs").join(FILE),
        }
    })
    .clone()
}

fn writable(path: &std::path::Path) -> bool {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .is_ok()
}

/// One line in the log (and on stderr).
pub fn line(text: impl AsRef<str>) {
    let text = text.as_ref();
    eprintln!("{text}");
    if !ACTIVE.load(Ordering::Relaxed) {
        return;
    }
    sa_client::diagnostics::append_line(&path(), text);
}

/// Start a session: rotate a big log and record how the game was started.
/// Join codes and other secrets are not written.
pub fn start(args: &[String]) {
    if cfg!(test) {
        return;
    }
    ACTIVE.store(true, Ordering::Relaxed);
    let log = path();
    if std::fs::metadata(&log).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(&log, log.with_extension("log.old"));
    }
    let mut shown = Vec::with_capacity(args.len());
    let mut hide_next = false;
    for arg in args {
        if hide_next {
            shown.push("[hidden]".to_owned());
            hide_next = false;
            continue;
        }
        hide_next = matches!(arg.as_str(), "--join-code" | "--password" | "--token");
        shown.push(arg.clone());
    }
    line(format!(
        "---- SARE runtime {} ({}) started: {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        shown.join(" ")
    ));
}
