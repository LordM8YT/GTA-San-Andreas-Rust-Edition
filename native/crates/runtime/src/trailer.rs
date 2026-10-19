//! Cinematic camera mode for trailers: `--trailer` flies scripted shots in
//! real time (record with OBS or Game Bar); `--trailer-record <dir>` renders
//! them at a fixed 60 FPS into one MP4 per shot through `ffmpeg` on PATH.
//! Shots use map coordinates and heights above the ground below them, so they
//! adapt to the player's own installation. `--trailer-shots <file>` replaces
//! the built-in list with lines in the same format as `SHOTS`.
use anyhow::{bail, Context, Result};
use glam::Vec3;
use std::{
    io::Write,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
};

/// One shot per line: name, map center x y, hour, weather index, seconds,
/// field of view, then camera start, camera end, look-at start and look-at
/// end, each as map x y and height above the ground there.
pub const SHOTS: &str = "\
grove_street_morning  2500 -1670  7  0  7  60  2440 -1660 3     2488 -1666 3     2515 -1682 2     2525 -1690 2
downtown_sunset       1480 -1730 19  1  8  55  1380 -1830 120   1580 -1830 120   1480 -1700 30    1480 -1700 30
vinewood_sign         1420  -900 18  0  7  50  1330  -980 40    1500  -980 40    1420  -805 10    1420  -805 10
santa_maria_beach      350 -1800 20  0  7  60   250 -1950 4      420 -1950 4      370 -2040 3      380 -2040 3
venturas_strip_night  2000  1500 23 11  8  60  2070  1200 60    2070  1650 60    2070  1780 20    2070  1850 10
san_fierro_rain      -2000   300 15  8  7  60 -2050   250 3    -1990   250 3    -1960   320 2    -1955   330 2
gant_bridge          -2680  1500 17  5  8  55 -2580  1400 80   -2580  1650 80   -2680  1550 40   -2680  1600 40
mount_chiliad        -2300 -1600  6 13  8  60 -2150 -1500 60   -2250 -1520 60   -2330 -1630 10   -2330 -1630 10
desert_sandstorm      -500  1900 13 19  6  60  -560  1850 10    -440  1850 10    -500  1950 5     -500  1950 5
grove_street_finale   2500 -1670 22  0  9  60  2495 -1720 10    2495 -1720 90    2495 -1680 2     2495 -1640 0
";
/// Frames of world simulation before a shot rolls, so traffic, streaming and
/// temporal history settle.
pub const SETTLE_FRAMES: u32 = 90;
pub const RECORD_FPS: u32 = 60;

#[derive(Clone, Debug, PartialEq)]
pub struct Shot {
    pub name: String,
    pub center: [f32; 2],
    pub hour: u8,
    pub weather: u8,
    pub seconds: f32,
    pub fov: f32,
    /// Camera start, camera end, look-at start, look-at end: map x, map y,
    /// height above ground.
    pub points: [[f32; 3]; 4],
}
pub fn parse(text: &str) -> Result<Vec<Shot>> {
    let mut shots = Vec::new();
    for (number, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.len() != 19 {
            bail!(
                "trailer shot line {} has {} fields, expected 19",
                number + 1,
                fields.len()
            );
        }
        let number_at = |i: usize| -> Result<f32> {
            fields[i]
                .parse::<f32>()
                .ok()
                .filter(|v| v.is_finite())
                .with_context(|| format!("line {}: invalid number {:?}", number + 1, fields[i]))
        };
        let hour = number_at(3)?;
        let weather = number_at(4)?;
        let seconds = number_at(5)?;
        if !(0.0..24.0).contains(&hour)
            || !(0.0..crate::settings::WEATHER_NAMES.len() as f32).contains(&weather)
            || !(0.5..=120.0).contains(&seconds)
        {
            bail!("line {}: hour, weather or length out of range", number + 1);
        }
        let mut points = [[0.0; 3]; 4];
        for (index, point) in points.iter_mut().enumerate() {
            for (axis, value) in point.iter_mut().enumerate() {
                *value = number_at(7 + index * 3 + axis)?;
            }
        }
        shots.push(Shot {
            name: fields[0].to_owned(),
            center: [number_at(1)?, number_at(2)?],
            hour: hour as u8,
            weather: weather as u8,
            seconds,
            fov: number_at(6)?.clamp(20.0, 110.0),
            points,
        });
    }
    if shots.is_empty() {
        bail!("no trailer shots");
    }
    Ok(shots)
}
/// Gentle start and stop, like a camera dolly.
fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
impl Shot {
    /// Camera eye, look-at target and field of view `seconds` into the shot.
    /// `ground` returns the surface height below a world-space point, if any.
    pub fn camera(&self, seconds: f32, ground: impl Fn(Vec3) -> Option<f32>) -> (Vec3, Vec3) {
        let t = ease(seconds / self.seconds);
        let place = |p: [f32; 3]| {
            let world = Vec3::new(p[0] - crate::ORIGIN[0], 0.0, crate::ORIGIN[1] - p[1]);
            Vec3::new(world.x, ground(world).unwrap_or(0.0) + p[2], world.z)
        };
        let [from, to, look_from, look_to] = self.points.map(place);
        let mut eye = from.lerp(to, t);
        // Never fly through a roof or hillside between the two ends.
        if let Some(surface) = ground(eye) {
            eye.y = eye.y.max(surface + 1.5);
        }
        (eye, look_from.lerp(look_to, t))
    }
}

/// Receives finished frames for one shot as tightly packed RGBA rows.
pub struct Recorder {
    child: Child,
    size: [u32; 2],
    pub path: PathBuf,
}
impl Recorder {
    pub fn start(directory: &Path, shot: usize, name: &str, size: [u32; 2]) -> Result<Self> {
        std::fs::create_dir_all(directory)?;
        let path = directory.join(format!("{:02}-{name}.mp4", shot + 1));
        let child = Command::new("ffmpeg")
            .args([
                "-y",
                "-loglevel",
                "error",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgba",
            ])
            .args(["-s", &format!("{}x{}", size[0], size[1])])
            .args(["-r", &RECORD_FPS.to_string(), "-i", "-"])
            // Even dimensions for yuv420p; near-lossless for later editing.
            .args(["-vf", "scale=trunc(iw/2)*2:trunc(ih/2)*2"])
            .args(["-c:v", "libx264", "-preset", "slow", "-crf", "14"])
            .args(["-pix_fmt", "yuv420p", "-movflags", "+faststart"])
            .arg(&path)
            .stdin(Stdio::piped())
            .spawn()
            .context(
                "trailer recording needs ffmpeg on PATH (for example: winget install ffmpeg)",
            )?;
        Ok(Self { child, size, path })
    }
    pub fn frame(&mut self, size: [u32; 2], rgba: &[u8]) -> Result<()> {
        if size != self.size {
            bail!("window size changed while recording; keep it fixed");
        }
        self.child
            .stdin
            .as_mut()
            .context("ffmpeg input closed")?
            .write_all(rgba)?;
        Ok(())
    }
    pub fn finish(mut self) -> Result<PathBuf> {
        drop(self.child.stdin.take());
        let status = self.child.wait()?;
        if !status.success() {
            bail!("ffmpeg failed for {}", self.path.display());
        }
        Ok(self.path)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    /// Waiting for the shot's neighbourhood to finish streaming in.
    Travel,
    /// Simulating with the camera in place before rolling.
    Settle(u32),
    /// Rolling the shot.
    Roll,
}
pub struct Director {
    pub shots: Vec<Shot>,
    pub index: usize,
    pub phase: Phase,
    /// `Some` renders at a fixed rate into MP4 files in this directory.
    pub record: Option<PathBuf>,
    pub recorder: Option<Recorder>,
    /// Seconds into the current shot, real time when not recording.
    pub seconds: f32,
    last_frame: Option<std::time::Instant>,
}
impl Director {
    pub fn from_args(args: &[String]) -> Result<Option<Self>> {
        let record = args
            .windows(2)
            .find(|pair| pair[0] == "--trailer-record")
            .map(|pair| PathBuf::from(&pair[1]));
        if record.is_none() && !args.iter().any(|arg| arg == "--trailer") {
            return Ok(None);
        }
        let shots = match args.windows(2).find(|pair| pair[0] == "--trailer-shots") {
            Some(pair) => parse(&std::fs::read_to_string(&pair[1])?)?,
            None => parse(SHOTS)?,
        };
        Ok(Some(Self {
            shots,
            index: 0,
            phase: Phase::Travel,
            record,
            recorder: None,
            seconds: 0.0,
            last_frame: None,
        }))
    }
    pub fn shot(&self) -> &Shot {
        &self.shots[self.index]
    }
    /// Moves to the next shot; false after the last one.
    pub fn next(&mut self) -> bool {
        self.index += 1;
        self.phase = Phase::Travel;
        self.seconds = 0.0;
        self.index < self.shots.len()
    }
}

impl crate::State {
    fn trailer_ground(&self, point: Vec3) -> Option<f32> {
        self.collision
            .as_ref()?
            .ground_below(Vec3::new(point.x, 1000.0, point.z), 1000.0)
    }
    /// Before a frame: travels to the current shot and places its camera.
    pub(super) fn direct_trailer(&mut self, director: &mut Director) -> Result<()> {
        let shot = director.shot().clone();
        self.menu.page = None;
        self.menu.has_played = true;
        self.menu.settings.show_hud = false;
        self.menu.settings.fixed_hour = Some(shot.hour);
        self.menu.settings.fixed_weather = Some(shot.weather);
        self.walking = false;
        self.driving = false;
        self.keys.clear();
        if director.record.is_some() {
            // Simulate exactly one recorded frame per rendered frame.
            self.last = std::time::Instant::now()
                - std::time::Duration::from_secs_f32(1.0 / RECORD_FPS as f32);
        }
        if director.phase == Phase::Travel {
            let arrived =
                crate::streaming::distance(self.region, shot.center) < 1.0 && self.interior == 0;
            if !arrived && self.destination.is_none() {
                self.interior_destination = None;
                self.destination = Some(shot.center);
            }
            if arrived && self.destination.is_none() && !self.loading() {
                println!("Trailer shot {}: {}", director.index + 1, shot.name);
                director.phase = Phase::Settle(0);
            }
        }
        let seconds = if director.phase == Phase::Roll {
            director.seconds
        } else {
            0.0
        };
        let (eye, target) = shot.camera(seconds, |p| self.trailer_ground(p));
        self.trailer_camera = Some((eye, target, shot.fov));
        if let (Phase::Roll, Some(directory)) = (director.phase, &director.record) {
            if director.recorder.is_none() {
                director.recorder = Some(Recorder::start(
                    directory,
                    director.index,
                    &shot.name,
                    [self.size.width, self.size.height],
                )?);
            }
            self.capture_raw = true;
        }
        Ok(())
    }
    /// After a rendered frame; returns false once every shot is done.
    pub(super) fn after_trailer_frame(&mut self, director: &mut Director) -> Result<bool> {
        let now = std::time::Instant::now();
        let dt = director
            .last_frame
            .replace(now)
            .map_or(0.0, |last| (now - last).as_secs_f32());
        match director.phase {
            Phase::Travel => {}
            Phase::Settle(frames) => {
                director.phase = if frames + 1 >= SETTLE_FRAMES {
                    director.seconds = 0.0;
                    Phase::Roll
                } else {
                    Phase::Settle(frames + 1)
                };
            }
            Phase::Roll => {
                if let (Some(recorder), Some((size, rgba))) =
                    (&mut director.recorder, self.captured_frame.take())
                {
                    recorder.frame(size, &rgba)?;
                }
                director.seconds += if director.record.is_some() {
                    1.0 / RECORD_FPS as f32
                } else {
                    dt
                };
                if director.seconds >= director.shot().seconds {
                    if let Some(recorder) = director.recorder.take() {
                        println!("Trailer clip saved: {}", recorder.finish()?.display());
                    }
                    if !director.next() {
                        self.trailer_camera = None;
                        return Ok(false);
                    }
                }
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn built_in_shots_parse_and_stay_near_their_neighbourhood() {
        let shots = parse(SHOTS).unwrap();
        assert_eq!(shots.len(), 10);
        for shot in &shots {
            for point in shot.points {
                let distance = (point[0] - shot.center[0]).hypot(point[1] - shot.center[1]);
                assert!(
                    distance < crate::streaming::RADIUS,
                    "{} leaves its streamed area: {distance}",
                    shot.name
                );
            }
        }
    }
    #[test]
    fn camera_eases_between_ends_and_stays_above_ground() {
        let shot = parse(
            "test 2500 -1670 12 0 2 60  2490 -1670 3  2510 -1670 3  2500 -1660 0  2500 -1660 0",
        )
        .unwrap()
        .remove(0);
        let flat = |_| Some(10.0);
        let (start, target) = shot.camera(0.0, flat);
        assert!(
            (start - Vec3::new(-10.0, 13.0, 0.0)).length() < 1e-4,
            "{start}"
        );
        assert!((target - Vec3::new(0.0, 10.0, -10.0)).length() < 1e-4);
        let (middle, _) = shot.camera(1.0, flat);
        assert!(middle.x.abs() < 1e-4);
        assert_eq!(shot.camera(2.0, flat).0, shot.camera(5.0, flat).0);
        // A rooftop higher than the planned path lifts the camera over it.
        let (over_roof, _) = shot.camera(1.0, |p: Vec3| {
            Some(if p.x.abs() < 1.0 { 30.0 } else { 10.0 })
        });
        assert!(over_roof.y >= 31.5);
    }
    #[test]
    fn recorder_writes_an_mp4_through_ffmpeg() {
        let encoders = Command::new("ffmpeg")
            .args(["-hide_banner", "-encoders"])
            .output();
        if !encoders.is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains("libx264")) {
            eprintln!("skipping: ffmpeg with libx264 not on PATH");
            return;
        }
        let directory = std::env::temp_dir().join(format!("sare-trailer-{}", std::process::id()));
        // Odd sizes are cropped to even ones for yuv420p.
        let size = [33, 17];
        let mut recorder = Recorder::start(&directory, 0, "test", size).unwrap();
        let frame = vec![200u8; (size[0] * size[1] * 4) as usize];
        for _ in 0..3 {
            recorder.frame(size, &frame).unwrap();
        }
        assert!(recorder.frame([32, 16], &frame).is_err());
        let path = recorder.finish().unwrap();
        assert!(path.ends_with("01-test.mp4"));
        assert!(std::fs::metadata(&path).unwrap().len() > 0);
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn bad_shot_lines_are_rejected() {
        assert!(parse("").is_err());
        assert!(parse("short 1 2 3").is_err());
        assert!(parse("late 0 0 25 0 2 60  0 0 1  1 1 1  2 2 2  3 3 3").is_err());
    }
}
