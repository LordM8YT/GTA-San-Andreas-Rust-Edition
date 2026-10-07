//! The PC SA geometry 2DFX roadsign record and particle font layout.
use super::{chunks, f32at, slice, u16at, u32at};
use anyhow::{ensure, Result};

#[derive(Clone, Debug)]
pub struct RoadSign {
    pub position: [f32; 3],
    pub size: [f32; 2],
    pub rotation: [f32; 3],
    pub flags: u16,
    pub text: [[u8; 16]; 4],
}
impl RoadSign {
    pub fn lines(&self) -> usize {
        match self.flags & 3 {
            0 => 4,
            n => n as usize,
        }
    }
    pub fn letters(&self) -> usize {
        match (self.flags >> 2) & 3 {
            0 => 16,
            n => 1 << n,
        }
    }
    pub fn color(&self) -> [f32; 4] {
        match (self.flags >> 4) & 3 {
            1 => [0.0, 0.0, 0.0, 1.0],
            2 => [128.0 / 255.0, 128.0 / 255.0, 128.0 / 255.0, 1.0],
            3 => [1.0, 0.0, 0.0, 1.0],
            _ => [1.0; 4],
        }
    }
}
pub(super) fn from_geometry(data: &[u8]) -> Result<Vec<RoadSign>> {
    let mut signs = Vec::new();
    for extension in chunks(data)?.into_iter().filter(|c| c.tag == 3) {
        for plugin in chunks(extension.body)?
            .into_iter()
            .filter(|c| c.tag == 0x0253_f2f8)
        {
            signs.extend(decode(plugin.body)?);
        }
    }
    Ok(signs)
}
fn decode(data: &[u8]) -> Result<Vec<RoadSign>> {
    let count = u32at(data, 0)? as usize;
    ensure!(count <= 4096, "2DFX effect budget exceeded");
    let mut offset = 4;
    let mut signs = Vec::new();
    for _ in 0..count {
        let header = slice(data, offset, 20)?;
        let size = u32at(header, 16)? as usize;
        offset += 20;
        let payload = slice(data, offset, size)?;
        offset += size;
        if u32at(header, 12)? != 7 {
            continue;
        }
        ensure!(size == 88, "invalid roadsign record length");
        let size = [f32at(payload, 0)?, f32at(payload, 4)?];
        ensure!(
            size.iter().all(|v| *v > 0.0 && *v <= 1000.0),
            "invalid roadsign dimensions"
        );
        let mut text = [[0; 16]; 4];
        for (row, line) in text.iter_mut().enumerate() {
            line.copy_from_slice(slice(payload, 22 + row * 16, 16)?);
        }
        signs.push(RoadSign {
            position: [f32at(header, 0)?, f32at(header, 4)?, f32at(header, 8)?],
            size,
            rotation: [f32at(payload, 8)?, f32at(payload, 12)?, f32at(payload, 16)?],
            flags: u16at(payload, 20)?,
            text,
        });
    }
    ensure!(offset == data.len(), "unexpected 2DFX tail");
    Ok(signs)
}
/// Cell in the original 4-column roadsignfont atlas. Underscores are spaces;
/// several punctuation characters encode arrows and transport pictograms.
pub fn glyph(byte: u8) -> Option<usize> {
    Some(match byte {
        b'A'..=b'Z' => usize::from(byte - b'A') + 24,
        b'a'..=b'z' => usize::from(byte - b'a') + 53,
        b'0'..=b'9' => usize::from(byte - b'0') + 11,
        b'!' => 0,
        b'&' => 2,
        b'(' => 4,
        b')' => 5,
        b'+' => 6,
        b'-' => 8,
        b'.' => 9,
        b';' => 21,
        b':' => 22,
        b'?' => 23,
        b'[' => 50,
        b']' => 52,
        b'<' => 82,
        b'>' => 83,
        b'^' => 84,
        b'~' => 85,
        b'%' => 86,
        b'#' => 87,
        b'$' => 88,
        b'*' => 89,
        b'@' => 90,
        b'|' => 91,
        b'{' => 92,
        b'}' => 94,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn record() -> Vec<u8> {
        let mut bytes = 1_u32.to_le_bytes().to_vec();
        for n in [10_f32, 20.0, 30.0] {
            bytes.extend(n.to_le_bytes());
        }
        bytes.extend(7_u32.to_le_bytes());
        bytes.extend(88_u32.to_le_bytes());
        for n in [4_f32, 2.0, 0.0, 90.0, 0.0] {
            bytes.extend(n.to_le_bytes());
        }
        bytes.extend(0x2e_u16.to_le_bytes()); // Two lines, eight columns, gray.
        bytes.extend(*b"GROVE_STREET____");
        bytes.extend(*b"Airport_}_#_____");
        bytes.extend([b'_'; 32]);
        bytes.extend([0; 2]);
        bytes
    }
    #[test]
    fn text_layout_palette_and_pictograms() {
        let signs = decode(&record()).unwrap();
        let s = &signs[0];
        assert_eq!(s.position, [10.0, 20.0, 30.0]);
        assert_eq!((s.lines(), s.letters()), (2, 8));
        assert_eq!(s.color()[0], 128.0 / 255.0);
        assert_eq!(s.text[0], *b"GROVE_STREET____");
        assert_eq!(glyph(b'A'), Some(24));
        assert_eq!(glyph(b'z'), Some(78));
        assert_eq!(glyph(b'}'), Some(94));
        assert_eq!(glyph(b'_'), None);
        for flags in 0..16 {
            let mut s = s.clone();
            s.flags = flags;
            assert_eq!(s.lines(), [4, 1, 2, 3][(flags & 3) as usize]);
            assert_eq!(s.letters(), [16, 2, 4, 8][(flags >> 2) as usize]);
        }
    }
    #[test]
    fn dff_extension_survives_frames_without_duplicating_shared_effects() {
        fn chunk(tag: u32, body: &[u8]) -> Vec<u8> {
            let mut out = tag.to_le_bytes().to_vec();
            out.extend((body.len() as u32).to_le_bytes());
            out.extend(0x1803ffff_u32.to_le_bytes());
            out.extend(body);
            out
        }
        let room = include_bytes!("../../../../mods/native-room-demo/room.dff");
        let mut body = Vec::new();
        for section in chunks(super::super::root(room, 16).unwrap()).unwrap() {
            let mut data = section.body.to_vec();
            match section.tag {
                1 => data[..4].copy_from_slice(&2_u32.to_le_bytes()),
                14 => {
                    data.clear();
                    for c in chunks(section.body).unwrap() {
                        let mut frame = c.body.to_vec();
                        if c.tag == 1 {
                            frame[40..44].copy_from_slice(&100_f32.to_le_bytes());
                        }
                        data.extend(chunk(c.tag, &frame));
                    }
                }
                26 => {
                    data.clear();
                    for c in chunks(section.body).unwrap() {
                        let mut geometry = c.body.to_vec();
                        if c.tag == 15 {
                            geometry.extend(chunk(3, &chunk(0x0253_f2f8, &record())));
                        }
                        data.extend(chunk(c.tag, &geometry));
                    }
                }
                _ => {}
            }
            body.extend(chunk(section.tag, &data));
            if section.tag == 20 {
                body.extend(chunk(20, &data));
            }
        }
        let decoded = super::super::decode_dff(&chunk(16, &body)).unwrap();
        assert_eq!(decoded.len(), 2);
        let signs: Vec<_> = decoded.iter().flat_map(|g| &g.road_signs).collect();
        assert_eq!(signs.len(), 1);
        assert_eq!(signs[0].position, [10.0, 20.0, 30.0]);
    }
    #[test]
    fn skips_other_effects_and_rejects_malformed_records() {
        let good = record();
        for end in 0..good.len() {
            assert!(decode(&good[..end]).is_err());
        }
        let mut mixed = 2_u32.to_le_bytes().to_vec();
        mixed.extend([0; 12]);
        mixed.extend(1_u32.to_le_bytes());
        mixed.extend(3_u32.to_le_bytes());
        mixed.extend([0; 3]);
        mixed.extend(&good[4..]);
        assert_eq!(decode(&mixed).unwrap().len(), 1);
        for (offset, value) in [(24, f32::NAN), (24, -1.0), (32, f32::INFINITY)] {
            let mut invalid = good.clone();
            invalid[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
            assert!(decode(&invalid).is_err());
        }
    }
}
