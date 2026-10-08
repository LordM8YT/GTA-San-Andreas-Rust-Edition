//! Read-only archive inspection. No original audio is exported.
fn main() -> anyhow::Result<()> {
    let game = std::env::args_os().nth(1).expect("game directory");
    let game = std::path::Path::new(&game);
    let archive = sa_audio::archive::SfxArchive::open(game)?;
    for bank in [0, 37, 86] {
        let sounds = archive.read_bank(bank)?;
        println!("bank {bank}: {} sounds", sounds.len());
        for (index, sound) in sounds.iter().enumerate().take(6) {
            println!(
                "  slot {index}: {} samples, {} Hz, loop {:?}, headroom {}",
                sound.samples.len(),
                sound.sample_rate,
                sound.loop_start,
                sound.headroom_hundredths_db
            );
        }
    }
    sa_audio::gameplay::GameplaySounds::load(game)?;
    println!("Gameplay sounds loaded from the user's installation");
    Ok(())
}
