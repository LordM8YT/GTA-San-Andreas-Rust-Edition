//! Read-only checks of the original PC data required by the free-roam path.
use sa_assets::{game_path, Img};
use std::{
    fs,
    path::{Path, PathBuf},
};
#[derive(Clone, Debug, Default)]
pub struct Validation {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}
impl Validation {
    pub fn ready(&self) -> bool {
        self.errors.is_empty()
    }
}
pub fn validate(root: &Path) -> Validation {
    let mut result = Validation::default();
    let Ok(root) = root.canonicalize() else {
        result.errors.push(
            "Installation folder does not exist. Choose your original PC San Andreas folder."
                .into(),
        );
        return result;
    };
    for required in [
        "models/gta3.img",
        "data/default.dat",
        "data/gta.dat",
        "anim/ped.ifp",
    ] {
        match game_path::resolve(&root, required).and_then(|p| {
            anyhow::ensure!(p.is_file(), "file missing");
            Ok(p)
        }) {
            Ok(path) => {
                if required.ends_with(".dat") {
                    match bounded_text(&path) {
                        Ok(text) => {
                            for line in text
                                .lines()
                                .map(|l| l.split('#').next().unwrap_or("").trim())
                            {
                                let mut fields = line.split_whitespace();
                                let Some(kind) = fields.next() else { continue };
                                if !kind.eq_ignore_ascii_case("IDE")
                                    && !kind.eq_ignore_ascii_case("IPL")
                                {
                                    continue;
                                }
                                if let Some(relative) = fields.next() {
                                    if !relative.to_ascii_lowercase().ends_with(
                                        if kind.eq_ignore_ascii_case("IDE") {
                                            ".ide"
                                        } else {
                                            ".ipl"
                                        },
                                    ) {
                                        continue;
                                    }
                                    if !game_path::resolve(&root, relative)
                                        .is_ok_and(|p| p.is_file())
                                    {
                                        result.errors.push(format!(
                                            "Missing or unsafe registered map file: {relative}"
                                        ));
                                    }
                                }
                            }
                        }
                        Err(_) => result
                            .errors
                            .push(format!("Cannot read {required}. Verify your installation.")),
                    }
                } else if required.ends_with(".img") {
                    match Img::open(&path) {
                        Ok(archive) => {
                            for model in ["taxi", "infernus", "admiral", "fam1", "fam2", "ballas1"]
                            {
                                for suffix in ["dff", "txd"] {
                                    let name = format!("{model}.{suffix}");
                                    if !archive.has(&name) {
                                        result.errors.push(format!(
                                            "models/gta3.img: missing or unsupported {name}"
                                        ));
                                    }
                                }
                            }
                        }
                        Err(e) => result
                            .errors
                            .push(format!("models/gta3.img is unsupported or damaged: {e}")),
                    }
                } else if let Err(e) = fs::read(&path)
                    .map_err(anyhow::Error::from)
                    .and_then(|b| sa_assets::ifp::decode(&b).map(|_| ()))
                {
                    result
                        .errors
                        .push(format!("anim/ped.ifp is unsupported or damaged: {e}"));
                }
            }
            Err(_) => result
                .errors
                .push(format!("Missing or inaccessible {required}")),
        }
    }
    for optional in [
        "models/gta_int.img",
        "models/particle.txd",
        "audio/CONFIG/PakFiles.dat",
        "audio/CONFIG/BankLkup.dat",
    ] {
        if !game_path::resolve(&root, optional).is_ok_and(|p| p.is_file()) {
            result.warnings.push(format!(
                "Optional {optional} missing: interiors, water/signs or sound may be unavailable."
            ));
        }
    }
    result.errors.truncate(128);
    result
}
fn bounded_text(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut text = String::new();
    fs::File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_string(&mut text)?;
    if text.len() > 1024 * 1024 {
        return Err(std::io::Error::other("DAT exceeds 1 MiB"));
    }
    Ok(text)
}
pub fn candidates() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(path) = std::env::var_os("GTA_SA_DIR") {
        paths.push(path.into());
    }
    if cfg!(windows) {
        for drive in ["C:", "D:", "E:"] {
            for suffix in [
                "Program Files (x86)/Steam/steamapps/common/Grand Theft Auto San Andreas",
                "Program Files (x86)/Rockstar Games/GTA San Andreas",
                "Program Files/Rockstar Games/GTA San Andreas",
                "SteamLibrary/steamapps/common/Grand Theft Auto San Andreas",
                "Games/GTA San Andreas",
                "GTA San Andreas/Grand Theft Auto San Andreas",
            ] {
                paths.push(PathBuf::from(format!("{drive}/{suffix}")));
            }
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        // Native, Debian-style and Flatpak Steam libraries.
        for steam in [
            ".steam/steam",
            ".local/share/Steam",
            ".steam/debian-installation",
            ".var/app/com.valvesoftware.Steam/.local/share/Steam",
        ] {
            paths.push(
                PathBuf::from(&home)
                    .join(steam)
                    .join("steamapps/common/Grand Theft Auto San Andreas"),
            );
        }
    }
    paths
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_and_unsupported_files_are_named_without_writing_to_installation() {
        let path = std::env::temp_dir().join(format!("sa-install-test-{}", std::process::id()));
        fs::create_dir_all(path.join("models")).unwrap();
        fs::write(path.join("models/gta3.img"), b"not-an-img").unwrap();
        let report = validate(&path);
        assert!(!report.ready());
        assert!(report.errors.iter().any(|e| e.contains("gta3.img")));
        assert!(report.errors.iter().any(|e| e.contains("ped.ifp")));
        assert!(!path.join("data").exists());
        assert_eq!(
            fs::read(path.join("models/gta3.img")).unwrap(),
            b"not-an-img"
        );
        fs::remove_file(path.join("models/gta3.img")).unwrap();
        fs::remove_dir(path.join("models")).unwrap();
        fs::remove_dir(path).unwrap();
    }
}
