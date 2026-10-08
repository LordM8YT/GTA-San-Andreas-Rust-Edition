use anyhow::{ensure, Result};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
};
#[derive(Clone, Debug, PartialEq)]
pub enum Session {
    Offline,
    Direct(String),
    Relay { address: String, code: String },
}
impl Session {
    /// Shared launch contract; unrelated development flags remain runtime-owned.
    pub fn from_args(args: &[String]) -> Result<Self> {
        let value = |flag: &str| -> Result<Option<String>> {
            let found: Vec<_> = args
                .iter()
                .enumerate()
                .filter(|(_, a)| a.as_str() == flag)
                .collect();
            ensure!(found.len() <= 1, "Duplicate {flag} argument");
            let Some((index, _)) = found.first() else {
                return Ok(None);
            };
            let next = args.get(index + 1).filter(|s| !s.starts_with("--"));
            Ok(Some(
                next.ok_or_else(|| anyhow::anyhow!("Missing value after {flag}"))?
                    .clone(),
            ))
        };
        let direct = value("--join")?;
        let code = value("--join-code")?;
        let relay = value("--relay-address")?;
        ensure!(
            direct.is_none() || code.is_none(),
            "Choose either direct IP or a relay join code"
        );
        ensure!(
            !(direct.is_some() || code.is_some())
                || !args.iter().any(|s| s == "--host" || s == "--relay-host"),
            "Choose either join or host"
        );
        if let Some(address) = direct {
            address.parse::<std::net::SocketAddr>()?;
            return Ok(Self::Direct(address));
        }
        if let Some(code) = code {
            let address = relay.unwrap_or_else(|| "127.0.0.1:7778".into());
            address.parse::<std::net::SocketAddr>()?;
            let code = code.trim().to_ascii_uppercase();
            ensure!(
                code.len() == 12 && code.bytes().all(|b| b.is_ascii_hexdigit()),
                "Use the 12-character join code"
            );
            return Ok(Self::Relay { address, code });
        }
        Ok(Self::Offline)
    }
}
#[derive(Clone, Debug)]
pub struct Launch {
    pub game: PathBuf,
    pub mods: PathBuf,
    pub cache: PathBuf,
    pub player: String,
    pub session: Session,
}
impl Launch {
    pub fn args(&self) -> Result<Vec<OsString>> {
        ensure!(
            self.player.chars().count() <= 24 && !self.player.chars().any(char::is_control),
            "Use a player name up to 24 characters"
        );
        let mut args: Vec<OsString> = vec![
            "--game-dir".into(),
            self.game.as_os_str().into(),
            "--mods-dir".into(),
            self.mods.as_os_str().into(),
            "--cache-dir".into(),
            self.cache.as_os_str().into(),
            "--name".into(),
            self.player.clone().into(),
            "--play".into(),
        ];
        match &self.session {
            Session::Offline => {}
            Session::Direct(address) => {
                address.parse::<std::net::SocketAddr>()?;
                args.extend(["--join".into(), address.into()]);
            }
            Session::Relay { address, code } => {
                address.parse::<std::net::SocketAddr>()?;
                let code = code.trim().to_ascii_uppercase();
                ensure!(
                    code.len() == 12 && code.bytes().all(|b| b.is_ascii_hexdigit()),
                    "Use the 12-character join code"
                );
                args.extend([
                    "--relay-address".into(),
                    address.into(),
                    "--join-code".into(),
                    code.into(),
                ]);
            }
        }
        Ok(args)
    }
    pub fn command(&self, exe: &Path) -> Result<Command> {
        ensure!(
            exe.is_file(),
            "Runtime is missing. Extract the complete SARE package beside the launcher."
        );
        let mut cmd = Command::new(exe);
        cmd.args(self.args()?);
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }
        if let Some(parent) = exe.parent() {
            cmd.current_dir(parent);
        }
        Ok(cmd)
    }
}
pub struct RuntimeGuard(pub fs::File);
impl RuntimeGuard {
    pub fn acquire() -> Result<Self> {
        Self::acquire_at(&super::config_dir())
    }
    fn acquire_at(profile: &Path) -> Result<Self> {
        fs::create_dir_all(profile)?;
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(profile.join("runtime.lock"))?;
        file.try_lock().map_err(|_| {
            anyhow::anyhow!(
                "SARE is already running. Close the current game before starting another."
            )
        })?;
        Ok(Self(file))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normal_runtime_profile_lock_rejects_duplicates_and_releases_on_exit() {
        let root = std::env::temp_dir().join(format!(
            "sa-launch-lock-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let first = RuntimeGuard::acquire_at(&root).unwrap();
        assert!(RuntimeGuard::acquire_at(&root).is_err());
        drop(first);
        drop(RuntimeGuard::acquire_at(&root).unwrap());
        fs::remove_file(root.join("runtime.lock")).unwrap();
        fs::remove_dir(root).unwrap();
    }
    #[test]
    fn paths_and_codes_are_separate_arguments_not_shell_commands() {
        let launch = Launch {
            game: PathBuf::from("C:/Games/SA & friends/$(secret)"),
            mods: "mods".into(),
            cache: "cache".into(),
            player: "A & B".into(),
            session: Session::Relay {
                address: "127.0.0.1:7778".into(),
                code: "abcdef123456".into(),
            },
        };
        let args = launch.args().unwrap();
        assert_eq!(args[1], launch.game.as_os_str());
        assert_eq!(args.last().unwrap(), "ABCDEF123456");
        assert!(args.contains(&"--play".into()));
        let text: Vec<_> = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(
            matches!(Session::from_args(&text).unwrap(),Session::Relay{code,..} if code=="ABCDEF123456")
        );
        assert!(Session::from_args(&["--join".into()]).is_err());
        assert!(Session::from_args(&[
            "--join".into(),
            "127.0.0.1:7777".into(),
            "--join-code".into(),
            "ABCDEF123456".into()
        ])
        .is_err());
        let mut invalid = launch;
        invalid.session = Session::Direct("0.0.0.0:not-a-port".into());
        assert!(invalid.args().is_err());
        invalid.session = Session::Relay {
            address: "127.0.0.1:7778".into(),
            code: "x\";bad".into(),
        };
        assert!(invalid.args().is_err());
    }
}
