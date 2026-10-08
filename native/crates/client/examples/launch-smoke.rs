//! Exercises the same structured Command used by the launcher with an isolated checkpoint.
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    anyhow::ensure!(
        args.len() == 4,
        "launch-smoke RUNTIME ORIGINAL_GAME TEST_PROFILE"
    );
    let profile = std::path::PathBuf::from(&args[3]);
    std::fs::create_dir_all(&profile)?;
    let launch = sa_client::launch::Launch {
        game: args[2].clone().into(),
        mods: profile.join("no-mods"),
        cache: profile.join("cache"),
        player: "Launcher test".into(),
        session: sa_client::launch::Session::Offline,
    };
    let mut command = launch.command(std::path::Path::new(&args[1]))?;
    command
        .env("SARE_CONFIG_DIR", &profile)
        .args(["--smoke-save", "--no-mods", "--save-file"])
        .arg(profile.join("progress.json"));
    let status = command.status()?;
    anyhow::ensure!(status.success(), "Runtime failed: {status}");
    anyhow::ensure!(
        sa_client::progress::Save::load(&profile.join("progress.json"))?.is_some(),
        "Checkpoint missing"
    );
    Ok(())
}
