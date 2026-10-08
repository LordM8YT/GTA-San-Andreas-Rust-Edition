use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Seek, SeekFrom},
    path::Path,
};
#[derive(Default, Serialize, Deserialize)]
pub struct Hardware {
    pub gpu: String,
    pub renderer: String,
}
pub fn safe_excerpt(text: &str) -> String {
    text.lines()
        .rev()
        .take(100)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|line| {
            let lower = line.to_ascii_lowercase();
            if line
                .as_bytes()
                .windows(12)
                .any(|w| w.iter().all(u8::is_ascii_hexdigit))
            {
                return "[private identifier line removed]".into();
            }
            if [
                "token",
                "credential",
                "password",
                "join code",
                "join-code",
                "authorization",
                "secret",
                ":\\",
                ":/",
                "/home/",
                "/users/",
                "/tmp/",
                "/run/",
                "/",
                "\\",
                "code=",
                "code:",
                "key=",
                "cookie",
                "bearer",
            ]
            .iter()
            .any(|pattern| lower.contains(pattern))
            {
                return "[private log line removed]".into();
            }
            line.split_whitespace()
                .map(|word| {
                    let plain = word.trim_matches(|c: char| !c.is_ascii_alphanumeric());
                    if (plain.len() == 12 && plain.bytes().all(|b| b.is_ascii_hexdigit()))
                        || (plain.len() >= 24
                            && plain
                                .bytes()
                                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'))
                    {
                        "[private identifier]".to_string()
                    } else {
                        word.to_string()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
pub fn tail(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let size = file.metadata()?.len();
    file.seek(SeekFrom::Start(size.saturating_sub(32 * 1024)))?;
    let mut bytes = Vec::new();
    file.take(32 * 1024).read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into())
}
pub fn report(log: &str, session: &str) -> String {
    let hardware: Hardware =
        super::read_json(&super::config_dir().join("runtime-status.json")).unwrap_or_default();
    format!("SARE diagnostic report\nClient: {}\nBuild: {} (local source changes may exist)\nOS: {} / {}\nLast observed GPU: {}\nLast observed renderer: {}\nSession: {}\n\nRecent log (redacted; review before sharing):\n{}\n",super::VERSION,super::BUILD_ID,std::env::consts::OS,std::env::consts::ARCH,safe_excerpt(&hardware.gpu),safe_excerpt(&hardware.renderer),safe_excerpt(session),safe_excerpt(log))
}
/// Short next steps; full local errors remain available in the runtime log.
pub fn explain(error: &str) -> String {
    let lower = error.to_ascii_lowercase();
    let next = if lower.contains("hash") || lower.contains("digest") {
        "Retry the download. Ask the host to verify the resource if it repeats."
    } else if lower.contains("cache")
        && (lower.contains("limit") || lower.contains("budget") || lower.contains("full"))
    {
        "Close sessions, then preview and clear unused downloads in Launcher > Resources."
    } else if lower.contains("version") || lower.contains("incompatible") {
        "Update the client, host and relay to the same build, then reconnect."
    } else if lower.contains("missing server mods") {
        "Enable required mod downloads in Settings, then reconnect."
    } else if lower.contains("cancel") {
        "Your offline setup is preserved. Join again when ready."
    } else if lower.contains("refused")
        || lower.contains("timed out")
        || lower.contains("unreachable")
    {
        "Check the IP and port, VPN connection and that the host or relay is running."
    } else if lower.contains("session not found") || lower.contains("request expired") {
        "Ask the host for a current join code and confirm the relay address, then retry."
    } else if lower.contains("session full") {
        "Wait for a free slot or choose another session."
    } else {
        "Check the local log for details, correct the problem and retry."
    };
    format!("{error} {next}")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn secrets_codes_and_personal_paths_are_removed() {
        let text="join code: ABCDEF123456\nC:\\Users\\Alice\\private.log\n/home/alice/file\ntoken=verysecret\nGPU upload 12 frames\nInvitation ABCDEF123456\n";
        let safe = safe_excerpt(text);
        for secret in ["ABCDEF123456", "Alice", "alice", "verysecret"] {
            assert!(!safe.contains(secret));
        }
        assert!(safe.contains("GPU upload 12 frames"));
    }
}
