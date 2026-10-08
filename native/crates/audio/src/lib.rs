//! Game-oriented audio services backed by Kira. The API hides backend handles
//! so the audio backend or San Andreas archive decoder can later be replaced.

use anyhow::{Context, Result};
use kira::{
    listener::ListenerHandle,
    sound::{
        static_sound::{StaticSoundData, StaticSoundHandle},
        streaming::{StreamingSoundData, StreamingSoundHandle},
    },
    track::{SpatialTrackBuilder, SpatialTrackHandle, TrackBuilder, TrackHandle},
    AudioManager, AudioManagerSettings, Decibels, DefaultBackend, Frame, Tween,
};
use std::path::{Path, PathBuf};
pub mod archive;
pub mod gameplay;

/// Two original looping samples on an effects-bus spatial child. Dropping the
/// emitter stops both voices and releases its mixer track.
pub struct EngineEmitter {
    track: SpatialTrackHandle,
    idle: StaticSoundHandle,
    rev: StaticSoundHandle,
}

impl EngineEmitter {
    fn new(
        effects: &mut TrackHandle,
        listener: &ListenerHandle,
        sounds: &gameplay::EngineSounds,
        position: [f32; 3],
    ) -> Result<Self> {
        validate_position(position, "engine")?;
        let mut track = effects.add_spatial_sub_track(
            listener,
            glam::Vec3::from_array(position),
            SpatialTrackBuilder::new()
                .distances((3.0, 65.0))
                .sound_capacity(2),
        )?;
        let idle = track.play(sounds.idle.data.volume(-14.0))?;
        let rev = track.play(sounds.rev.data.volume(Decibels::SILENCE))?;
        Ok(Self { track, idle, rev })
    }

    /// Native speed/throttle approximation, not the original game's gearbox.
    pub fn update(&mut self, position: [f32; 3], speed: f32, throttle: f32) -> Result<()> {
        validate_position(position, "engine")?;
        anyhow::ensure!(
            speed.is_finite() && throttle.is_finite(),
            "engine inputs must be finite"
        );
        let motion = (speed.abs() / 48.0).clamp(0.0, 1.0);
        let load = throttle.abs().clamp(0.0, 1.0);
        let blend = (motion * 0.75 + load * 0.45).clamp(0.0, 1.0);
        let tween = Tween {
            duration: std::time::Duration::from_millis(80),
            ..Tween::default()
        };
        let gain = |amplitude: f32| Decibels::from(20.0 * amplitude.max(0.001).log10());
        self.track
            .set_position(glam::Vec3::from_array(position), Tween::default());
        self.idle
            .set_volume(gain(0.20 * (1.0 - blend * 0.8)), tween);
        self.rev.set_volume(gain(0.20 * blend), tween);
        self.idle
            .set_playback_rate(f64::from(0.9 + motion * 0.35 + load * 0.1), tween);
        self.rev
            .set_playback_rate(f64::from(0.8 + motion * 0.65 + load * 0.25), tween);
        Ok(())
    }
}

impl Drop for EngineEmitter {
    fn drop(&mut self) {
        self.idle.stop(Tween::default());
        self.rev.stop(Tween::default());
    }
}

/// In-memory sound used for overlapping short effects.
#[derive(Clone, Debug)]
pub struct SoundEffect {
    data: StaticSoundData,
}
#[derive(Clone, Copy)]
pub enum MenuSound {
    Back,
    Highlight,
    Select,
}
pub struct FrontendSounds {
    back: SoundEffect,
    highlight: SoundEffect,
    select: SoundEffect,
}
impl FrontendSounds {
    /// Original frontend bank 60; sounds are stored as paired mono channels.
    pub fn load(game: &Path) -> Result<Self> {
        let sounds = archive::SfxArchive::open(game)?.read_bank(60)?;
        anyhow::ensure!(
            sounds.len() >= 8,
            "original frontend bank has fewer than eight sounds"
        );
        Ok(Self {
            back: SoundEffect::from_pcm_pair(&sounds[0], &sounds[1])?,
            highlight: SoundEffect::from_pcm_pair(&sounds[4], &sounds[5])?,
            select: SoundEffect::from_pcm_pair(&sounds[6], &sounds[7])?,
        })
    }
    pub fn sound(&self, kind: MenuSound) -> &SoundEffect {
        match kind {
            MenuSound::Back => &self.back,
            MenuSound::Highlight => &self.highlight,
            MenuSound::Select => &self.select,
        }
    }
}

impl SoundEffect {
    pub fn from_pcm(sound: &archive::PcmSound) -> Self {
        Self {
            data: StaticSoundData {
                sample_rate: sound.sample_rate,
                frames: sound
                    .samples
                    .iter()
                    .map(|sample| Frame::from_mono(f32::from(*sample) / 32768.0))
                    .collect::<Vec<_>>()
                    .into(),
                settings: Default::default(),
                slice: None,
            },
        }
    }
    pub fn from_pcm_pair(left: &archive::PcmSound, right: &archive::PcmSound) -> Result<Self> {
        anyhow::ensure!(
            left.sample_rate == right.sample_rate,
            "stereo SFX rates differ"
        );
        let frames = (0..left.samples.len().max(right.samples.len()))
            .map(|index| Frame {
                left: f32::from(left.samples.get(index).copied().unwrap_or(0)) / 32768.0,
                right: f32::from(right.samples.get(index).copied().unwrap_or(0)) / 32768.0,
            })
            .collect::<Vec<_>>();
        Ok(Self {
            data: StaticSoundData {
                sample_rate: left.sample_rate,
                frames: frames.into(),
                settings: Default::default(),
                slice: None,
            },
        })
    }
    /// Loads a short WAV, OGG, or FLAC file into memory.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let data = StaticSoundData::from_file(path)
            .with_context(|| format!("load sound effect {}", path.display()))?;
        Ok(Self { data })
    }

    /// Builds a bounded test tone without requiring audio files or a device.
    pub fn tone(frequency_hz: f32, duration_seconds: f32, sample_rate: u32) -> Result<Self> {
        anyhow::ensure!(
            frequency_hz.is_finite() && frequency_hz > 0.0,
            "frequency must be finite and positive"
        );
        anyhow::ensure!(
            duration_seconds.is_finite() && duration_seconds > 0.0,
            "duration must be finite and positive"
        );
        anyhow::ensure!(
            (4000..=192000).contains(&sample_rate),
            "unsupported test-tone sample rate"
        );
        anyhow::ensure!(
            frequency_hz <= sample_rate as f32 / 2.0,
            "test-tone frequency exceeds Nyquist limit"
        );
        anyhow::ensure!(
            duration_seconds <= 10.0,
            "tone duration must not exceed 10 seconds"
        );
        let frame_count = (duration_seconds * sample_rate as f32).ceil() as usize;
        let frames = (0..frame_count)
            .map(|index| {
                let time = index as f32 / sample_rate as f32;
                let fade = 1.0 - index as f32 / frame_count as f32;
                Frame::from_mono((std::f32::consts::TAU * frequency_hz * time).sin() * 0.2 * fade)
            })
            .collect::<Vec<_>>();
        Ok(Self {
            data: StaticSoundData {
                sample_rate,
                frames: frames.into(),
                settings: Default::default(),
                slice: None,
            },
        })
    }
}

/// File-backed music, decoded as a stream when playback starts.
#[derive(Clone, Debug)]
pub struct MusicTrack {
    path: PathBuf,
}

impl MusicTrack {
    /// Opens a music file for streaming playback (WAV, OGG, or FLAC).
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        anyhow::ensure!(
            path.is_file(),
            "music file does not exist: {}",
            path.display()
        );
        Ok(Self { path })
    }
}

/// Live game audio. Dropping the engine releases the output device.
pub struct AudioEngine {
    manager: AudioManager<DefaultBackend>,
    music: TrackHandle,
    music_handle: Option<StreamingSoundHandle<kira::sound::FromFileError>>,
    effects: TrackHandle,
    spatial: Vec<SpatialTrackHandle>,
    listener: ListenerHandle,
}

impl AudioEngine {
    pub fn engine_emitter(
        &mut self,
        sounds: &gameplay::EngineSounds,
        position: [f32; 3],
    ) -> Result<EngineEmitter> {
        EngineEmitter::new(&mut self.effects, &self.listener, sounds, position)
    }
    /// Opens the operating system's default output device.
    pub fn new() -> Result<Self> {
        let mut manager = AudioManager::<DefaultBackend>::new(AudioManagerSettings::default())
            .context("initialize audio output")?;
        let music = manager
            .add_sub_track(TrackBuilder::new().volume(-6.0).sound_capacity(4))
            .context("create music mixer")?;
        let effects = manager
            .add_sub_track(TrackBuilder::new().sound_capacity(64))
            .context("create effects mixer")?;
        let listener = manager
            .add_listener(glam::Vec3::ZERO, glam::Quat::IDENTITY)
            .context("create spatial audio listener")?;
        Ok(Self {
            manager,
            music,
            music_handle: None,
            effects,
            spatial: Vec::new(),
            listener,
        })
    }

    /// Plays a short effect; effects on this track can overlap.
    pub fn play_effect(&mut self, sound: &SoundEffect) -> Result<()> {
        self.effects
            .play(sound.data.clone())
            .context("play sound effect")?;
        Ok(())
    }

    /// Starts a music stream, optionally looping the entire file.
    pub fn play_music(&mut self, music: &MusicTrack, looping: bool) -> Result<()> {
        let mut data = StreamingSoundData::from_file(&music.path)
            .with_context(|| format!("open music stream {}", music.path.display()))?;
        if looping {
            data = data.loop_region(..);
        }
        let handle = self.music.play(data).context("play music stream")?;
        self.music_handle = Some(handle);
        Ok(())
    }

    /// Stops the current music stream with a short fade.
    pub fn stop_music(&mut self) {
        if let Some(handle) = &mut self.music_handle {
            handle.stop(Tween {
                duration: std::time::Duration::from_millis(250),
                ..Tween::default()
            });
        }
        self.music_handle = None;
    }

    /// Sets music-bus gain in decibels.
    pub fn set_mix_levels(&mut self, master: f32, music: f32, effects: f32) -> Result<()> {
        anyhow::ensure!(
            [master, music, effects]
                .into_iter()
                .all(|v| v.is_finite() && (0.0..=1.0).contains(&v)),
            "audio levels must be finite values between zero and one"
        );
        let gain = |level: f32| {
            if level == 0.0 {
                Decibels::SILENCE
            } else {
                Decibels::from(20.0 * level.log10())
            }
        };
        self.manager
            .main_track()
            .set_volume(gain(master), Tween::default());
        self.music.set_volume(gain(music), Tween::default());
        self.effects.set_volume(gain(effects), Tween::default());
        Ok(())
    }
    /// Sets music-bus gain in decibels.
    pub fn set_music_volume(&mut self, decibels: f32) -> Result<()> {
        anyhow::ensure!(decibels.is_finite(), "music volume must be finite");
        self.music
            .set_volume(Decibels::from(decibels.clamp(-60.0, 6.0)), Tween::default());
        Ok(())
    }

    /// Sets effects-bus gain in decibels.
    pub fn set_effects_volume(&mut self, decibels: f32) -> Result<()> {
        anyhow::ensure!(decibels.is_finite(), "effects volume must be finite");
        self.effects
            .set_volume(Decibels::from(decibels.clamp(-60.0, 6.0)), Tween::default());
        Ok(())
    }

    /// Plays a short effect at a world-space position with distance attenuation.
    pub fn play_spatial(&mut self, sound: &SoundEffect, position: [f32; 3]) -> Result<()> {
        validate_position(position, "sound")?;
        self.spatial.retain(|track| track.num_sounds() > 0);
        anyhow::ensure!(self.spatial.len() < 64, "too many active spatial emitters");
        let mut emitter = self
            .effects
            .add_spatial_sub_track(
                &self.listener,
                glam::Vec3::from_array(position),
                SpatialTrackBuilder::new()
                    .distances((1.0, 80.0))
                    .sound_capacity(4),
            )
            .context("create spatial sound emitter")?;
        emitter
            .play(sound.data.clone())
            .context("play spatial sound")?;
        self.spatial.push(emitter);
        Ok(())
    }

    /// Updates listener position/orientation; Kira's default forward direction is -Z.
    pub fn set_listener(&mut self, position: [f32; 3], orientation: [f32; 4]) -> Result<()> {
        validate_position(position, "listener")?;
        anyhow::ensure!(
            orientation.iter().all(|value| value.is_finite()),
            "listener orientation must be finite"
        );
        let rotation = glam::Quat::from_xyzw(
            orientation[0],
            orientation[1],
            orientation[2],
            orientation[3],
        );
        anyhow::ensure!(
            rotation.length_squared() > f32::EPSILON,
            "listener orientation must not be zero"
        );
        self.listener
            .set_position(glam::Vec3::from_array(position), Tween::default());
        self.listener
            .set_orientation(rotation.normalize(), Tween::default());
        Ok(())
    }

    /// Sets master output gain in decibels.
    pub fn set_master_volume(&mut self, decibels: f32) -> Result<()> {
        anyhow::ensure!(decibels.is_finite(), "volume must be finite");
        self.manager
            .main_track()
            .set_volume(Decibels::from(decibels.clamp(-60.0, 6.0)), Tween::default());
        Ok(())
    }

    /// Number of short effects currently playing on the effects track.
    pub fn active_effects(&self) -> usize {
        self.effects.num_sounds()
    }
}

fn validate_position(position: [f32; 3], label: &str) -> Result<()> {
    anyhow::ensure!(
        position.iter().all(|value| value.is_finite()),
        "{label} position must be finite"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use kira::{
        backend::mock::{MockBackend, MockBackendSettings},
        AudioManagerSettings,
    };

    fn mock_manager() -> AudioManager<MockBackend> {
        AudioManager::new(AudioManagerSettings {
            backend_settings: MockBackendSettings {
                sample_rate: 48_000,
            },
            ..AudioManagerSettings::default()
        })
        .expect("mock audio backend needs no device")
    }

    fn tone(frequency: f32, sample_count: usize) -> StaticSoundData {
        let frames = (0..sample_count)
            .map(|index| {
                let sample =
                    (std::f32::consts::TAU * frequency * index as f32 / 48_000.0).sin() * 0.2;
                Frame::from_mono(sample)
            })
            .collect::<Vec<_>>();
        StaticSoundData {
            sample_rate: 48_000,
            frames: frames.into(),
            settings: Default::default(),
            slice: None,
        }
    }

    #[test]
    fn synthetic_pcm_has_expected_duration_and_bounded_samples() {
        let pcm = tone(440.0, 2400);
        assert_eq!(pcm.frames.len(), 2400);
        assert!(pcm.frames.iter().all(|frame| frame.left.is_finite()
            && frame.right.is_finite()
            && frame.left.abs() <= 0.2
            && frame.right.abs() <= 0.2));
    }
    #[test]
    fn original_pcm_stereo_keeps_channels_rates_and_shorter_channel_padding() {
        let left = archive::PcmSound {
            sample_rate: 16000,
            samples: vec![i16::MIN, i16::MAX],
            loop_start: None,
            headroom_hundredths_db: 0,
        };
        let mut right = archive::PcmSound {
            sample_rate: 16000,
            samples: vec![16384],
            loop_start: None,
            headroom_hundredths_db: 0,
        };
        let sound = SoundEffect::from_pcm_pair(&left, &right).unwrap();
        assert_eq!(sound.data.sample_rate, 16000);
        assert_eq!(sound.data.frames.len(), 2);
        assert_eq!(sound.data.frames[0].left, -1.0);
        assert_eq!(sound.data.frames[0].right, 0.5);
        assert_eq!(sound.data.frames[1].right, 0.0);
        right.sample_rate = 8000;
        assert!(SoundEffect::from_pcm_pair(&left, &right).is_err());
        assert!(SoundEffect::tone(440.0, 10.0, u32::MAX).is_err());
    }

    #[test]
    fn mock_backend_mixes_overlapping_sounds_without_hardware() {
        let mut manager = mock_manager();
        let mut effects = manager
            .add_sub_track(TrackBuilder::new().sound_capacity(2))
            .unwrap();
        let sound = tone(440.0, 2400);
        let _first = effects.play(sound.clone()).unwrap();
        let _second = effects.play(sound).unwrap();
        assert_eq!(effects.num_sounds(), 2);
        manager.backend_mut().on_start_processing();
        manager.backend_mut().process();
        assert_eq!(effects.num_sounds(), 2);
    }

    #[test]
    fn mock_backend_processes_spatial_emitter_with_listener() {
        let mut manager = mock_manager();
        let listener = manager
            .add_listener(glam::Vec3::ZERO, glam::Quat::IDENTITY)
            .unwrap();
        let mut emitter = manager
            .add_spatial_sub_track(
                &listener,
                glam::Vec3::X,
                SpatialTrackBuilder::new().distances((1.0, 80.0)),
            )
            .unwrap();
        let _sound = emitter.play(tone(220.0, 960)).unwrap();
        emitter.set_position(glam::Vec3::Y, Tween::default());
        manager.backend_mut().on_start_processing();
        manager.backend_mut().process();
        assert_eq!(emitter.num_sounds(), 1);
    }

    #[test]
    fn world_positions_must_be_finite() {
        assert!(validate_position([0.0, f32::INFINITY, 0.0], "sound").is_err());
        assert!(validate_position([0.0, 1.0, 0.0], "sound").is_ok());
    }

    // Capture actual mixer output: this checks parent-bus mute and release of
    // looping emitters without relying on an operating-system audio device.
    struct CaptureBackend {
        renderer: Option<kira::backend::Renderer>,
        buffer: Vec<f32>,
    }
    impl kira::backend::Backend for CaptureBackend {
        type Settings = ();
        type Error = ();
        fn setup(_: (), frames: usize) -> std::result::Result<(Self, u32), ()> {
            Ok((
                Self {
                    renderer: None,
                    buffer: vec![0.0; frames * 2],
                },
                48_000,
            ))
        }
        fn start(&mut self, renderer: kira::backend::Renderer) -> std::result::Result<(), ()> {
            self.renderer = Some(renderer);
            Ok(())
        }
    }
    impl CaptureBackend {
        fn energy(&mut self, blocks: usize) -> f32 {
            let renderer = self.renderer.as_mut().unwrap();
            let mut energy = 0.0;
            for _ in 0..blocks {
                renderer.on_start_processing();
                renderer.process(&mut self.buffer, 2);
                energy += self
                    .buffer
                    .iter()
                    .map(|sample| sample * sample)
                    .sum::<f32>();
            }
            energy
        }
    }
    #[test]
    fn engine_loops_follow_effects_mute_and_release_on_drop() {
        let mut manager = AudioManager::<CaptureBackend>::new(AudioManagerSettings {
            backend_settings: (),
            ..AudioManagerSettings::default()
        })
        .unwrap();
        let mut effects = manager.add_sub_track(TrackBuilder::new()).unwrap();
        let listener = manager
            .add_listener(glam::Vec3::ZERO, glam::Quat::IDENTITY)
            .unwrap();
        let sample = SoundEffect {
            data: tone(220.0, 960).loop_region(..),
        };
        let sounds = gameplay::EngineSounds {
            idle: sample.clone(),
            rev: sample,
        };
        let mut engine =
            EngineEmitter::new(&mut effects, &listener, &sounds, [0.0, 0.0, -2.0]).unwrap();
        engine.update([0.0, 0.0, -2.0], 20.0, 1.0).unwrap();
        assert!(engine.update([f32::NAN, 0.0, 0.0], 0.0, 0.0).is_err());
        assert!(engine.update([0.0; 3], f32::INFINITY, 0.0).is_err());
        assert!(
            manager.backend_mut().energy(100) > 0.01,
            "loops must survive past sample end"
        );
        effects.set_volume(Decibels::SILENCE, Tween::default());
        manager.backend_mut().energy(30);
        assert!(
            manager.backend_mut().energy(30) < 1e-10,
            "engine bypassed effects mute"
        );
        effects.set_volume(0.0, Tween::default());
        manager.backend_mut().energy(30);
        assert!(manager.backend_mut().energy(30) > 0.01);
        drop(engine);
        manager.backend_mut().energy(30);
        assert!(
            manager.backend_mut().energy(30) < 1e-10,
            "dropped engine kept looping"
        );
    }
}
