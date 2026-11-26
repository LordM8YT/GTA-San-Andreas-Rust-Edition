//! Friends and online status through a SARE directory (`sa_net::directory`).
//! The launcher sends a signed heartbeat about once a minute while it is
//! open, and shows the friends who are online and have added the player
//! back. Without a directory the page still shows the friend code and the
//! list, so codes can be swapped before a directory exists.
use sa_net::directory::{self, Heartbeat, Playing, Reply};
use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

/// The official directory, once it runs. `SARE_DIRECTORY` or the setting
/// override it.
pub const DEFAULT_DIRECTORY: &str = "";

pub struct Friends {
    key: Option<ed25519_dalek::SigningKey>,
    /// This player's friend code, or why there is none.
    pub code: Result<String, String>,
    /// The last answer: friends online now, keyed by code.
    pub online: Vec<directory::Friend>,
    pub error: Option<String>,
    pub last: Option<Instant>,
    worker: Option<mpsc::Receiver<Result<Reply, String>>>,
    /// What the game the launcher started is in, shown to friends.
    pub playing: Option<Playing>,
}
impl Friends {
    pub fn new(secret: Result<String, String>) -> Self {
        let key = secret.as_deref().ok().map(directory::sare_id);
        Self {
            code: match &key {
                Some(key) => Ok(directory::friend_code(&key.verifying_key())),
                None => Err(secret.err().unwrap_or_default()),
            },
            key,
            online: Vec::new(),
            error: None,
            last: None,
            worker: None,
            playing: None,
        }
    }
    pub fn busy(&self) -> bool {
        self.worker.is_some()
    }
    /// Collect a finished heartbeat.
    pub fn poll(&mut self) {
        let Some(worker) = &self.worker else { return };
        let outcome = match worker.try_recv() {
            Ok(outcome) => outcome,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => Err("Heartbeat task stopped.".into()),
        };
        self.worker = None;
        match outcome {
            Ok(reply) => {
                self.online = reply.online;
                self.error = None;
            }
            Err(error) => {
                self.online.clear();
                self.error = Some(error);
            }
        }
    }
    /// Send a heartbeat when one is due (or `now`), in the background.
    pub fn tick(&mut self, url: &str, name: &str, friends: Vec<String>, now: bool) {
        let due = self
            .last
            .is_none_or(|at| at.elapsed() >= directory::HEARTBEAT_EVERY);
        if url.is_empty() || self.worker.is_some() || !(due || now) {
            return;
        }
        let Some(key) = &self.key else { return };
        self.last = Some(Instant::now());
        let heartbeat = Heartbeat {
            key: sa_net::identity::hex(key.verifying_key().as_bytes()),
            name: name.to_owned(),
            time: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            playing: self.playing.clone(),
            friends,
        };
        let signed = directory::sign(key, &heartbeat);
        let url = format!("{}/v1/heartbeat", url.trim_end_matches('/'));
        let (tx, rx) = mpsc::channel();
        self.worker = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(send(&url, &signed));
        });
    }
}

fn send(url: &str, signed: &directory::Signed) -> Result<Reply, String> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        // A local test directory runs on http://127.0.0.1.
        .https_only(!url.starts_with("http://127.0.0.1") && !url.starts_with("http://localhost"))
        .timeout_global(Some(Duration::from_secs(10)))
        .http_status_as_error(false)
        .build()
        .into();
    let mut response = agent
        .post(url)
        .header("Content-Type", "application/json")
        .send(serde_json::to_vec(signed).map_err(|e| e.to_string())?)
        .map_err(|e| format!("The SARE directory could not be reached: {e}"))?;
    let status = response.status();
    let body = response
        .body_mut()
        .with_config()
        .limit(256 * 1024)
        .read_to_vec()
        .map_err(|e| e.to_string())?;
    if !status.is_success() {
        let reason = serde_json::from_slice::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| v.get("error")?.as_str().map(str::to_owned))
            .unwrap_or_else(|| format!("HTTP {status}"));
        return Err(format!("The SARE directory said: {reason}"));
    }
    serde_json::from_slice(&body).map_err(|_| "The SARE directory sent an invalid answer.".into())
}

/// The directory URL in use: the setting, `SARE_DIRECTORY`, or the default.
pub fn directory_url(setting: &str) -> String {
    let setting = setting.trim();
    if !setting.is_empty() {
        return setting.to_owned();
    }
    std::env::var("SARE_DIRECTORY")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_DIRECTORY.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_launchers_see_each_other_once_both_added_the_other() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || directory::serve(listener));
        let mut alice = Friends::new(Ok("alice-secret".into()));
        let mut bob = Friends::new(Ok("bob-secret".into()));
        let (a, b) = (alice.code.clone().unwrap(), bob.code.clone().unwrap());
        bob.playing = Some(Playing {
            world: "Harbor RP".into(),
            server: Some("127.0.0.1:7777".into()),
        });
        let wait = |f: &mut Friends| {
            let start = Instant::now();
            while f.busy() {
                f.poll();
                assert!(start.elapsed() < Duration::from_secs(10));
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        bob.tick(&url, "Bob", vec![a.clone()], true);
        wait(&mut bob);
        assert!(bob.error.is_none(), "{:?}", bob.error);
        alice.tick(&url, "Alice", vec![b.clone()], true);
        wait(&mut alice);
        assert_eq!(alice.online.len(), 1);
        assert_eq!(alice.online[0].name, "Bob");
        assert_eq!(
            alice.online[0].playing.as_ref().unwrap().server.as_deref(),
            Some("127.0.0.1:7777")
        );
        // Not due again yet, so nothing is sent.
        alice.tick(&url, "Alice", vec![b], false);
        assert!(!alice.busy());
    }

    #[test]
    fn unreachable_directories_report_an_error() {
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", closed.local_addr().unwrap());
        drop(closed);
        let mut me = Friends::new(Ok("me".into()));
        me.tick(&url, "Me", vec![], true);
        let start = Instant::now();
        while me.busy() {
            me.poll();
            assert!(start.elapsed() < Duration::from_secs(15));
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(me.error.is_some());
    }
}
