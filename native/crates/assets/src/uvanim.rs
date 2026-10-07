//! Bounded primary-channel RenderWare UV tracks used by original SA signs.
use super::{chunks, f32at, one, slice, u32at};
use anyhow::{ensure, Result};
use std::{collections::HashMap, sync::Arc};

#[derive(Clone, Debug)]
pub struct UvAnimation {
    pub name: String,
    duration: f32,
    parametric: bool,
    keys: Vec<(f32, [f32; 6])>,
}
impl UvAnimation {
    /// Affine UV matrix [right.x, right.y, up.x, up.y, translation.x, translation.y].
    pub fn matrix(&self, seconds: f32) -> [f32; 6] {
        let time = if seconds.is_finite() {
            seconds.rem_euclid(self.duration)
        } else {
            0.0
        };
        let after = self.keys.partition_point(|k| k.0 <= time);
        let a = &self.keys[after.saturating_sub(1)];
        let b = &self.keys[after.min(self.keys.len() - 1)];
        let t = if b.0 > a.0 {
            (time - a.0) / (b.0 - a.0)
        } else {
            0.0
        };
        let mut p: [f32; 6] = std::array::from_fn(|i| a.1[i] + (b.1[i] - a.1[i]) * t);
        if !self.parametric {
            return p;
        }
        let delta = (b.1[0] - a.1[0]).sin().atan2((b.1[0] - a.1[0]).cos());
        p[0] = a.1[0] + delta * t;
        let (s, c) = p[0].sin_cos();
        // Scale/skew/translate, then rotate about the texture center.
        [
            c * p[1] - s * p[3],
            s * p[1] + c * p[3],
            -s * p[2],
            c * p[2],
            c * (p[4] - 0.5) - s * (p[5] - 0.5) + 0.5,
            s * (p[4] - 0.5) + c * (p[5] - 0.5) + 0.5,
        ]
    }
    pub fn transform(matrix: [f32; 6], uv: [f32; 2]) -> [f32; 2] {
        [
            uv[0] * matrix[0] + uv[1] * matrix[2] + matrix[4],
            uv[0] * matrix[1] + uv[1] * matrix[3] + matrix[5],
        ]
    }
}
fn text(bytes: &[u8]) -> Result<String> {
    let end = bytes.iter().position(|b| *b == 0).unwrap_or(bytes.len());
    let s = std::str::from_utf8(&bytes[..end])?;
    ensure!(
        !s.is_empty() && s.bytes().all(|b| b.is_ascii() && !b.is_ascii_control()),
        "invalid UV animation name"
    );
    Ok(s.to_ascii_lowercase())
}
pub fn from_dff(mut data: &[u8]) -> Result<HashMap<String, Arc<UvAnimation>>> {
    let mut out = HashMap::new();
    let mut count = 0;
    while u32at(data, 0)? == 43 {
        ensure!(count < 16, "DFF UV dictionary budget");
        let body = super::root(data, 43)?;
        out.extend(dictionary(body)?);
        data = &data[12 + body.len()..];
        count += 1;
    }
    Ok(out)
}
pub(super) fn dictionary(data: &[u8]) -> Result<HashMap<String, Arc<UvAnimation>>> {
    let count = u32at(one(data, 1)?, 0)? as usize;
    ensure!(count <= 256, "UV animation dictionary budget");
    let mut out = HashMap::new();
    for entry in chunks(data)?.into_iter().filter(|c| c.tag == 27) {
        let d = entry.body;
        let kind = u32at(d, 4)?;
        let count = u32at(d, 8)? as usize;
        let duration = f32at(d, 16)?;
        ensure!(
            u32at(d, 0)? == 0x100 && matches!(kind, 0x1c0 | 0x1c1),
            "unsupported UV interpolation"
        );
        ensure!(
            (2..=65536).contains(&count)
                && d.len() == 88 + count * 32
                && duration > 0.0
                && duration <= 86400.0,
            "invalid UV keyframe bounds"
        );
        // Other serialized node slots can contain stale exporter pointers.
        // SA's shipped tracks use node zero on UV channel zero.
        ensure!(u32at(d, 56)? == 0, "unsupported UV channel");
        let name = text(slice(d, 24, 32)?)?;
        let mut chains: Vec<Vec<(f32, [f32; 6])>> = Vec::new();
        let mut chain_for_frame = Vec::with_capacity(count);
        for i in 0..count {
            let p = 88 + i * 32;
            let time = f32at(d, p)?;
            let previous = super::i32at(d, p + 28)?;
            let chain = if previous >= 0 && (previous as usize) < i {
                chain_for_frame[previous as usize]
            } else {
                ensure!(time == 0.0 && chains.len() < 8, "invalid UV node root");
                chains.push(Vec::new());
                chains.len() - 1
            };
            ensure!(
                time >= 0.0
                    && time <= duration + 0.001
                    && chains[chain].last().is_none_or(|k| time >= k.0),
                "unordered UV track"
            );
            chain_for_frame.push(chain);
            let mut values = [0.0; 6];
            for (j, value) in values.iter_mut().enumerate() {
                *value = f32at(d, p + 4 + j * 4)?;
            }
            chains[chain].push((time, values));
        }
        let keys = chains.remove(0);
        ensure!(
            keys[0].0 == 0.0 && (keys.last().unwrap().0 - duration).abs() < 0.001,
            "incomplete UV timeline"
        );
        let animation = Arc::new(UvAnimation {
            name: name.clone(),
            duration,
            parametric: kind == 0x1c1,
            keys,
        });
        ensure!(out.insert(name, animation).is_none(), "duplicate UV track");
    }
    ensure!(out.len() == count, "UV dictionary count mismatch");
    Ok(out)
}
pub(super) fn material(
    data: &[u8],
    tracks: &HashMap<String, Arc<UvAnimation>>,
) -> Result<Option<Arc<UvAnimation>>> {
    for ext in chunks(data)?.into_iter().filter(|c| c.tag == 3) {
        if let Some(plugin) = chunks(ext.body)?.into_iter().find(|c| c.tag == 0x135) {
            let s = one(plugin.body, 1)?;
            ensure!(
                s.len() == 36 && u32at(s, 0)? == 1,
                "unsupported UV material slots"
            );
            let name = text(slice(s, 4, 32)?)?;
            // Standalone models may refer to another DFF's dictionary. Keep
            // their static mesh when the caller has no shared world index.
            return Ok(tracks.get(&name).cloned());
        }
    }
    Ok(None)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn chunk(tag: u32, body: &[u8]) -> Vec<u8> {
        let mut out = tag.to_le_bytes().to_vec();
        out.extend((body.len() as u32).to_le_bytes());
        out.extend(0x1803ffff_u32.to_le_bytes());
        out.extend(body);
        out
    }
    #[test]
    fn parses_interleaved_nodes_and_material_binding_and_checks_bounds() {
        let mut animation = vec![0; 88];
        for (offset, value) in [(0, 0x100_u32), (4, 0x1c1), (8, 4)] {
            animation[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        animation[16..20].copy_from_slice(&2_f32.to_le_bytes());
        animation[24..27].copy_from_slice(b"own");
        for (time, y_scale, y, previous) in [
            (0.0_f32, 1.0, 0.0, -100_i32),
            (0.0, -1.0, 1.0, -100),
            (2.0, 1.0, 2.0, 0),
            (2.0, -1.0, -1.0, 1),
        ] {
            for value in [time, 0.0, 1.0, y_scale, 0.0, 0.0, y] {
                animation.extend(value.to_le_bytes());
            }
            animation.extend(previous.to_le_bytes());
        }
        let mut dictionary_body = chunk(1, &1_u32.to_le_bytes());
        dictionary_body.extend(chunk(27, &animation));
        let mut dff = chunk(43, &dictionary_body);
        dff.extend(chunk(16, &[]));
        let tracks = from_dff(&dff).unwrap();
        assert_eq!(
            UvAnimation::transform(tracks["own"].matrix(1.0), [0.25, 0.5]),
            [0.25, 1.5]
        );
        let mut binding = 1_u32.to_le_bytes().to_vec();
        binding.extend(b"own");
        binding.extend([0; 29]);
        let material_bytes = chunk(3, &chunk(0x135, &chunk(1, &binding)));
        assert!(Arc::ptr_eq(
            &material(&material_bytes, &tracks).unwrap().unwrap(),
            &tracks["own"]
        ));
        assert!(material(&material_bytes, &HashMap::new())
            .unwrap()
            .is_none());
        for end in 0..dictionary_body.len() {
            assert!(dictionary(&dictionary_body[..end]).is_err());
        }
        animation[88..92].copy_from_slice(&f32::NAN.to_le_bytes());
        let mut invalid = chunk(1, &1_u32.to_le_bytes());
        invalid.extend(chunk(27, &animation));
        assert!(dictionary(&invalid).is_err());
    }
    #[test]
    fn scrolling_wraps_without_accumulation_and_rotates_around_center() {
        let mut track = UvAnimation {
            name: "own".into(),
            duration: 2.0,
            parametric: true,
            keys: vec![
                (0.0, [0.0, 1.0, 1.0, 0.0, 0.0, 0.0]),
                (2.0, [0.0, 1.0, 1.0, 0.0, 1.0, 0.0]),
            ],
        };
        assert_eq!(
            UvAnimation::transform(track.matrix(1.0), [0.25, 0.75]),
            [0.75, 0.75]
        );
        assert_eq!(track.matrix(1.0), track.matrix(3.0));
        track.keys[1].1 = [std::f32::consts::FRAC_PI_2, 1.0, 1.0, 0.0, 0.0, 0.0];
        let result = UvAnimation::transform(track.matrix(1.0), [0.5, 0.5]);
        assert!((result[0] - 0.5).abs() < 1e-6 && (result[1] - 0.5).abs() < 1e-6);
        track.parametric = false;
        track.keys = vec![
            (0.0, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]),
            (2.0, [1.0, 0.0, 0.0, 1.0, 0.0, 1.0]),
        ];
        assert_eq!(
            UvAnimation::transform(track.matrix(1.0), [0.25, 0.75]),
            [0.25, 1.25]
        );
    }
}
