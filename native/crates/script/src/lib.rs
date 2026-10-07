//! A deliberately small SCM interpreter core and original cutscene inventory.
//! Main.scm is not run until its segments and required opcodes are validated.
use anyhow::{bail, ensure, Context, Result};
use sa_assets::Img;
use std::path::Path;

#[derive(Debug, PartialEq, Eq)]
pub enum Step {
    Wait(u32),
    Jump(usize),
    End,
}
pub struct Vm<'a> {
    code: &'a [u8],
    pc: usize,
    waiting_until: u64,
    ended: bool,
}
impl<'a> Vm<'a> {
    pub fn new(code: &'a [u8]) -> Self {
        Self {
            code,
            pc: 0,
            waiting_until: 0,
            ended: false,
        }
    }
    fn integer(&mut self) -> Result<i32> {
        let kind = *self.code.get(self.pc).context("truncated SCM operand")?;
        self.pc += 1;
        let size = match kind {
            1 => 4,
            4 => 1,
            _ => bail!("unsupported SCM operand type {kind:#x}"),
        };
        let bytes = self
            .code
            .get(self.pc..self.pc + size)
            .context("truncated SCM integer")?;
        self.pc += size;
        Ok(if size == 1 {
            i8::from_le_bytes([bytes[0]]) as i32
        } else {
            i32::from_le_bytes(bytes.try_into()?)
        })
    }
    pub fn tick(&mut self, now_ms: u64, budget: usize) -> Result<Vec<Step>> {
        if self.ended || now_ms < self.waiting_until {
            return Ok(Vec::new());
        }
        ensure!(budget > 0 && budget <= 10_000, "invalid script step budget");
        let mut events = Vec::new();
        for _ in 0..budget {
            let op = self
                .code
                .get(self.pc..self.pc + 2)
                .context("truncated SCM opcode")?;
            let opcode = u16::from_le_bytes(op.try_into()?);
            self.pc += 2;
            match opcode {
                0x0001 => {
                    let ms = self.integer()?;
                    ensure!(
                        (0..=60_000).contains(&ms),
                        "SCM wait outside supported bounds"
                    );
                    self.waiting_until = now_ms + ms as u64;
                    events.push(Step::Wait(ms as u32));
                    return Ok(events);
                }
                0x0002 => {
                    let target = self.integer()?;
                    ensure!(
                        target >= 0 && (target as usize) < self.code.len(),
                        "SCM jump outside script"
                    );
                    self.pc = target as usize;
                    events.push(Step::Jump(self.pc));
                }
                0x004e => {
                    self.ended = true;
                    events.push(Step::End);
                    return Ok(events);
                }
                _ => bail!("unsupported SCM opcode {opcode:04x} at {}", self.pc - 2),
            }
        }
        bail!("SCM instruction budget exceeded")
    }
}

#[derive(Debug)]
pub struct Cutscene {
    pub name: String,
    pub offset: [f32; 3],
    pub models: Vec<String>,
    pub subtitles: usize,
    pub animation_bytes: usize,
    pub camera_tracks: usize,
    pub model_assets_present: usize,
    pub audio_archive_present: bool,
}
fn cut_text(data: &[u8]) -> Result<&str> {
    let end = data.iter().position(|b| *b == 0).unwrap_or(data.len());
    Ok(std::str::from_utf8(&data[..end])?)
}
fn parse_cut(name: &str, data: &[u8]) -> Result<([f32; 3], Vec<String>, usize)> {
    let mut section = "";
    let mut offset = None;
    let mut models = Vec::new();
    let mut subtitles = 0;
    for line in cut_text(data)?
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if line.eq_ignore_ascii_case("end") {
            section = "";
            continue;
        }
        if section.is_empty() {
            section = line;
            continue;
        }
        match section {
            "info" if line.starts_with("offset ") => {
                let f: Vec<_> = line[7..].split_whitespace().collect();
                ensure!(f.len() == 3, "invalid cutscene offset");
                let p: [f32; 3] = [f[0].parse()?, f[1].parse()?, f[2].parse()?];
                ensure!(p.iter().all(|v| v.is_finite()), "nonfinite cutscene offset");
                offset = Some(p);
            }
            "model" => {
                let f: Vec<_> = line.split(',').map(str::trim).collect();
                ensure!(
                    f.len() == 3 && f[0] == "1",
                    "unsupported cutscene model row"
                );
                models.push(f[1].to_ascii_lowercase());
                ensure!(models.len() <= 128, "too many cutscene models");
            }
            "text" => subtitles += 1,
            _ => {}
        }
    }
    Ok((
        offset.with_context(|| format!("{name}: missing offset"))?,
        models,
        subtitles,
    ))
}
fn camera_tracks(data: &[u8]) -> Result<usize> {
    let s = cut_text(data)?;
    let mut tracks = 0;
    for group in s.split(';') {
        let lines: Vec<_> = group
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        if lines.is_empty() {
            continue;
        }
        let count: usize = lines[0]
            .trim_end_matches(',')
            .parse()
            .context("invalid camera track count")?;
        ensure!(
            count > 0 && count <= 10_000 && lines.len() == count + 1,
            "invalid camera track length"
        );
        for line in &lines[1..] {
            let time = line
                .split(',')
                .next()
                .context("camera key")?
                .trim_end_matches('f')
                .parse::<f32>()?;
            ensure!(time.is_finite() && time >= 0.0, "invalid camera key time");
        }
        tracks += 1;
        ensure!(tracks <= 64, "too many camera tracks");
    }
    Ok(tracks)
}
pub fn inspect_cutscene(game: &Path, name: &str) -> Result<Cutscene> {
    ensure!(
        !name.is_empty()
            && name.len() <= 32
            && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'),
        "invalid cutscene name"
    );
    let game = game.canonicalize()?;
    let mut cuts = Img::open(&game.join("anim/cuts.img"))?;
    let cut = cuts.read(&format!("{name}.cut"))?;
    let dat = cuts.read(&format!("{name}.dat"))?;
    let ifp = cuts.read(&format!("{name}.ifp"))?;
    ensure!(
        ifp.len() >= 8 && &ifp[..4] == b"ANPK",
        "unsupported cutscene animation header"
    );
    let declared = u32::from_le_bytes(ifp[4..8].try_into()?) as usize;
    ensure!(declared <= ifp.len() - 8, "invalid IFP length");
    let (offset, models, subtitles) = parse_cut(name, &cut)?;
    let camera_tracks = camera_tracks(&dat)?;
    let assets = Img::open(&game.join("models/cutscene.img"))?;
    let present = models
        .iter()
        .filter(|m| assets.has(&format!("{m}.dff")))
        .count();
    Ok(Cutscene {
        name: name.to_owned(),
        offset,
        models,
        subtitles,
        animation_bytes: declared,
        camera_tracks,
        model_assets_present: present,
        audio_archive_present: game.join("audio/streams/CUTSCENE").is_file(),
    })
}

#[derive(Clone, Debug)]
pub struct CutKey {
    pub seconds: f32,
    pub rotation: [f32; 4],
    pub translation: [f32; 3],
}
#[derive(Clone, Debug)]
pub struct CutAnimation {
    pub keys: Vec<CutKey>,
    pub camera: [f32; 3],
    pub target: [f32; 3],
    pub fov: f32,
    pub camera_keys: Option<Vec<CameraKey>>,
    pub height_offset: f32,
}
#[derive(Clone, Debug)]
pub struct CameraKey {
    pub seconds: f32,
    pub position: [f32; 3],
    pub target: [f32; 3],
    pub fov: f32,
}
fn ifp_chunks(data: &[u8]) -> Result<Vec<([u8; 4], &[u8])>> {
    let mut p = 0;
    let mut out = Vec::new();
    while p < data.len() {
        ensure!(data.len() - p >= 8, "truncated IFP chunk");
        let tag = data[p..p + 4].try_into()?;
        let n = u32::from_le_bytes(data[p + 4..p + 8].try_into()?) as usize;
        let end = p.checked_add(8 + n).context("IFP size overflow")?;
        ensure!(end <= data.len(), "IFP chunk beyond input");
        out.push((tag, &data[p + 8..end]));
        p = p
            .checked_add(8 + n.div_ceil(4) * 4)
            .context("IFP padded size overflow")?;
        ensure!(p <= data.len(), "IFP padding beyond input");
    }
    Ok(out)
}
fn camera_row(line: &str) -> Result<Vec<f32>> {
    let mut row = Vec::new();
    for field in line.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let value = field.trim_end_matches('f').parse::<f32>()?;
        ensure!(value.is_finite(), "nonfinite camera value");
        row.push(value);
    }
    Ok(row)
}
/// The original `cuttest` is small enough to validate its root animation and camera tracks.
/// This is an asset preview, not general SA cutscene playback.
pub fn load_cuttest_animation(game: &Path) -> Result<CutAnimation> {
    let mut cuts = Img::open(&game.join("anim/cuts.img"))?;
    let dat = cuts.read("cuttest.dat")?;
    let ifp = cuts.read("cuttest.ifp")?;
    let text = cut_text(&dat)?;
    let tracks: Vec<Vec<_>> = text
        .split(';')
        .filter_map(|g| {
            let rows: Vec<_> = g.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
            if rows.is_empty() {
                None
            } else {
                Some(rows)
            }
        })
        .collect();
    ensure!(tracks.len() == 4, "cuttest camera layout changed");
    let row = |track: usize| -> Result<Vec<f32>> {
        camera_row(tracks[track].get(1).context("missing camera key")?)
    };
    let fov = row(0)?.get(1).copied().context("missing FOV")?;
    let camera = row(2)?;
    let target = row(3)?;
    ensure!(
        camera.len() >= 4 && target.len() >= 4 && fov > 1.0 && fov < 179.0,
        "unsupported cuttest camera"
    );
    ensure!(ifp.len() >= 8 && &ifp[..4] == b"ANPK", "missing ANPK");
    let length = u32::from_le_bytes(ifp[4..8].try_into()?) as usize;
    ensure!(length <= ifp.len() - 8, "invalid ANPK size");
    let root = &ifp[8..8 + length];
    let mut keys = Vec::new();
    let mut group_name = String::new();
    for (tag, body) in ifp_chunks(root)? {
        if &tag == b"NAME" {
            group_name = String::from_utf8_lossy(body)
                .trim_end_matches('\0')
                .to_ascii_lowercase();
        }
        if &tag != b"DGAN" || group_name != "csgoldrec" {
            continue;
        }
        for (t, pan) in ifp_chunks(body)? {
            if &t != b"CPAN" {
                continue;
            }
            let chunks = ifp_chunks(pan)?;
            let bone = chunks
                .iter()
                .find(|(t, _)| t == b"ANIM")
                .context("missing animation bone")?
                .1;
            if !bone.starts_with(b"DummyRoot\0") {
                continue;
            }
            let raw = chunks
                .iter()
                .find(|(t, _)| t == b"KRT0")
                .context("missing cuttest root keys")?
                .1;
            ensure!(
                raw.len() % 32 == 0 && raw.len() <= 32 * 1000,
                "invalid cuttest key data"
            );
            for frame in raw.as_chunks::<32>().0.iter() {
                let f = |p: usize| f32::from_le_bytes(frame[p..p + 4].try_into().unwrap());
                let key = CutKey {
                    rotation: [f(0), f(4), f(8), f(12)],
                    translation: [f(16), f(20), f(24)],
                    seconds: f(28),
                };
                ensure!(
                    key.rotation
                        .iter()
                        .chain(key.translation.iter())
                        .chain(std::iter::once(&key.seconds))
                        .all(|v| v.is_finite()),
                    "nonfinite cuttest key"
                );
                keys.push(key);
            }
        }
    }
    ensure!(
        !keys.is_empty() && keys.windows(2).all(|w| w[0].seconds <= w[1].seconds),
        "missing or unordered cuttest root keys"
    );
    Ok(CutAnimation {
        keys,
        camera: [camera[1], camera[2], camera[3]],
        target: [target[1], target[2], target[3]],
        fov,
        camera_keys: None,
        height_offset: 0.0,
    })
}

/// A bounded preview of the opening scene's original taxi root and camera tracks.
/// Character skinning, audio and SCM events are outside this preview.
pub fn load_prologue_animation(game: &Path) -> Result<CutAnimation> {
    let mut cuts = Img::open(&game.join("anim/cuts.img"))?;
    let dat = cuts.read("prolog1.dat")?;
    let ifp = cuts.read("prolog1.ifp")?;
    let tracks: Vec<Vec<Vec<f32>>> = cut_text(&dat)?
        .split(';')
        .filter_map(|group| {
            let lines: Vec<_> = group
                .lines()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect();
            (!lines.is_empty()).then_some(lines)
        })
        .map(|lines| -> Result<_> {
            let count: usize = lines[0].trim_end_matches(',').parse()?;
            ensure!(
                count > 0 && count <= 1000 && lines.len() == count + 1,
                "invalid prologue track"
            );
            lines[1..].iter().map(|line| camera_row(line)).collect()
        })
        .collect::<Result<_>>()?;
    ensure!(tracks.len() == 4, "unsupported prologue camera layout");
    let count = tracks[0].len();
    ensure!(
        tracks.iter().all(|t| t.len() == count),
        "camera track count mismatch"
    );
    let sample = |track: &[Vec<f32>], seconds: f32| -> Result<[f32; 3]> {
        ensure!(
            track.iter().all(|row| row.len() >= 4),
            "short camera vector track"
        );
        let after = track.partition_point(|row| row[0] <= seconds);
        let a = &track[after.saturating_sub(1)];
        let b = &track[after.min(track.len() - 1)];
        let factor = if b[0] > a[0] {
            ((seconds - a[0]) / (b[0] - a[0])).clamp(0.0, 1.0)
        } else {
            0.0
        };
        Ok(std::array::from_fn(|j| {
            a[j + 1] + (b[j + 1] - a[j + 1]) * factor
        }))
    };
    let mut camera_keys = Vec::with_capacity(count);
    for i in 0..count {
        let position = &tracks[2][i];
        ensure!(position.len() >= 4, "short camera key");
        let seconds = position[0];
        let fov = tracks[0]
            .iter()
            .take_while(|row| row[0] <= seconds)
            .last()
            .context("missing FOV key")?[1];
        ensure!((1.0..179.0).contains(&fov), "invalid prologue FOV");
        camera_keys.push(CameraKey {
            seconds,
            fov,
            position: [position[1], position[2], position[3]],
            target: sample(&tracks[3], seconds)?,
        });
    }
    ensure!(
        camera_keys.windows(2).all(|w| w[0].seconds <= w[1].seconds),
        "unordered camera keys"
    );
    ensure!(
        ifp.len() >= 8 && &ifp[..4] == b"ANPK",
        "invalid prologue IFP"
    );
    let declared = u32::from_le_bytes(ifp[4..8].try_into()?) as usize;
    ensure!(declared <= ifp.len() - 8, "invalid prologue IFP size");
    let mut group = String::new();
    let mut keys = Vec::new();
    for (tag, body) in ifp_chunks(&ifp[8..8 + declared])? {
        if &tag == b"NAME" {
            group = String::from_utf8_lossy(body)
                .trim_end_matches('\0')
                .to_ascii_lowercase();
        }
        if &tag != b"DGAN" || group != "cstaxi92" {
            continue;
        }
        for (tag, pan) in ifp_chunks(body)? {
            if &tag != b"CPAN" {
                continue;
            }
            let chunks = ifp_chunks(pan)?;
            let Some(bone) = chunks.iter().find(|(t, _)| t == b"ANIM").map(|(_, b)| *b) else {
                continue;
            };
            if !bone.starts_with(b"taxi\0") {
                continue;
            }
            let raw = chunks
                .iter()
                .find(|(t, _)| t == b"KRT0")
                .context("missing taxi root keys")?
                .1;
            ensure!(
                raw.len() % 32 == 0 && raw.len() <= 32 * 1000,
                "invalid taxi key data"
            );
            for frame in raw.as_chunks::<32>().0.iter() {
                let f = |p: usize| f32::from_le_bytes(frame[p..p + 4].try_into().unwrap());
                let key = CutKey {
                    rotation: [f(0), f(4), f(8), f(12)],
                    translation: [f(16), f(20), f(24)],
                    seconds: f(28),
                };
                let norm = key.rotation.iter().map(|v| v * v).sum::<f32>();
                if (0.5..1.5).contains(&norm)
                    && key
                        .translation
                        .iter()
                        .all(|v| v.is_finite() && v.abs() < 10_000.0)
                    && key.seconds.is_finite()
                {
                    keys.push(key);
                }
            }
        }
    }
    ensure!(
        !keys.is_empty() && keys.windows(2).all(|w| w[0].seconds <= w[1].seconds),
        "missing or unordered taxi root keys"
    );
    Ok(CutAnimation {
        camera: camera_keys[0].position,
        target: camera_keys[0].target,
        fov: camera_keys[0].fov,
        camera_keys: Some(camera_keys),
        height_offset: 0.0,
        keys,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wait_jump_and_end() {
        let data = [1, 0, 4, 5, 2, 0, 1, 12, 0, 0, 0, 0, 0x4e, 0];
        let mut vm = Vm::new(&data);
        assert_eq!(vm.tick(100, 8).unwrap(), vec![Step::Wait(5)]);
        assert!(vm.tick(104, 8).unwrap().is_empty());
        assert_eq!(vm.tick(105, 8).unwrap(), vec![Step::Jump(12), Step::End]);
    }
    #[test]
    fn unknown_opcode_stops() {
        let mut vm = Vm::new(&[0xff, 0x7f]);
        assert!(vm.tick(0, 1).is_err());
    }
    #[test]
    fn cutscene_metadata() {
        let c = b"info\noffset 1 2 3\nend\nmodel\n1, prop, prop\nend\ntext\n0,100,KEY\nend\n";
        assert_eq!(
            parse_cut("x", c).unwrap(),
            ([1.0, 2.0, 3.0], vec!["prop".into()], 1)
        );
        assert_eq!(camera_tracks(b"2,\n0f,1,2,3,\n1f,4,5,6,\n;\n").unwrap(), 1);
    }
}
