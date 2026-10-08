//! Original classic-PC samples; no audio files are bundled with the runtime.
use crate::{archive::SfxArchive, SoundEffect};
use anyhow::{ensure, Result};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineKind {
    Sedan,
    Sports,
}

pub struct EngineSounds {
    pub(crate) idle: SoundEffect,
    pub(crate) rev: SoundEffect,
}

pub struct GameplaySounds {
    sedan: EngineSounds,
    sports: EngineSounds,
    footsteps: Vec<SoundEffect>,
}

impl GameplaySounds {
    pub fn load(game: &Path) -> Result<Self> {
        let archive = SfxArchive::open(game)?;
        // Classic GENRL: MERC_D=86 (Taxi/Admiral), COBRA_D=37 (Infernus).
        // Within both engine banks, 0 is rev and 1 is idle.
        let engines = |bank| -> Result<EngineSounds> {
            let samples = archive.read_bank(bank)?;
            ensure!(samples.len() >= 2, "engine bank {bank} is incomplete");
            Ok(EngineSounds {
                rev: loop_sample(&samples[0])?,
                idle: loop_sample(&samples[1])?,
            })
        };
        let feet = archive.read_bank(0)?;
        ensure!(feet.len() >= 6, "generic footsteps bank is incomplete");
        Ok(Self {
            sedan: engines(86)?,
            sports: engines(37)?,
            // Generic/concrete footsteps are slots 1..=5; slot 0 is not a step.
            footsteps: feet[1..=5]
                .iter()
                .map(|pcm| {
                    let mut sound = SoundEffect::from_pcm(pcm);
                    sound.data = sound.data.volume(-12.0);
                    sound
                })
                .collect(),
        })
    }

    pub fn engine(&self, kind: EngineKind) -> &EngineSounds {
        match kind {
            EngineKind::Sedan => &self.sedan,
            EngineKind::Sports => &self.sports,
        }
    }

    pub fn footstep(&self, index: usize) -> &SoundEffect {
        &self.footsteps[index % self.footsteps.len()]
    }
}

fn loop_sample(pcm: &crate::archive::PcmSound) -> Result<SoundEffect> {
    ensure!(
        !pcm.samples.is_empty() && pcm.sample_rate > 0,
        "empty engine sample"
    );
    let start = pcm.loop_start.unwrap_or(0);
    ensure!(
        start < pcm.samples.len(),
        "engine loop begins past sample end"
    );
    let mut sound = SoundEffect::from_pcm(pcm);
    sound.data = sound
        .data
        .loop_region(start as f64 / f64::from(pcm.sample_rate)..);
    Ok(sound)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn archive_loop_keeps_intro_and_rejects_out_of_bounds_start() {
        let mut pcm = crate::archive::PcmSound {
            sample_rate: 8000,
            samples: vec![1; 100],
            loop_start: Some(20),
            headroom_hundredths_db: 0,
        };
        let sound = loop_sample(&pcm).unwrap();
        assert_eq!(sound.data.frames.len(), 100);
        assert!(sound.data.settings.loop_region.is_some());
        pcm.loop_start = Some(100);
        assert!(loop_sample(&pcm).is_err());
        pcm.samples.clear();
        assert!(loop_sample(&pcm).is_err());
    }
}
