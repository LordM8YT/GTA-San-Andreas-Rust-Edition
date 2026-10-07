//! COL1/2/3 collision containers. Offsets in COL2/3 are relative to byte 4.
//! Format reference: https://gtamods.com/wiki/Collision_File
use crate::{f32at, name, slice, u16at, u32at};
use anyhow::{ensure, Context, Result};

pub struct CollisionModel {
    pub name: String,
    pub triangles: Vec<[[f32; 3]; 3]>,
    pub min: [f32; 3],
    pub max: [f32; 3],
}
fn vector(d: &[u8], p: usize) -> Result<[f32; 3]> {
    Ok([f32at(d, p)?, f32at(d, p + 4)?, f32at(d, p + 8)?])
}
fn add_box(triangles: &mut Vec<[[f32; 3]; 3]>, min: [f32; 3], max: [f32; 3]) -> Result<()> {
    ensure!((0..3).all(|i| min[i] <= max[i]), "invalid COL box");
    let vertices: Vec<_> = (0..8)
        .map(|n| std::array::from_fn(|a| if n & (1 << a) == 0 { min[a] } else { max[a] }))
        .collect();
    for [a, b, c, e] in [
        [0, 1, 3, 2],
        [4, 6, 7, 5],
        [0, 4, 5, 1],
        [2, 3, 7, 6],
        [0, 2, 6, 4],
        [1, 5, 7, 3],
    ] {
        triangles.push([vertices[a], vertices[b], vertices[c]]);
        triangles.push([vertices[a], vertices[c], vertices[e]]);
    }
    Ok(())
}
fn add_sphere(out: &mut Vec<[[f32; 3]; 3]>, center: [f32; 3], radius: f32) -> Result<()> {
    ensure!((0.0..=1000.0).contains(&radius), "invalid COL sphere");
    let vertex = |ring: usize, segment: usize| {
        let lat = std::f32::consts::PI * ring as f32 / 8.0;
        let lon = std::f32::consts::TAU * segment as f32 / 16.0;
        [
            center[0] + radius * lat.sin() * lon.cos(),
            center[1] + radius * lat.sin() * lon.sin(),
            center[2] + radius * lat.cos(),
        ]
    };
    for ring in 0..8 {
        for segment in 0..16 {
            let a = vertex(ring, segment);
            let b = vertex(ring, segment + 1);
            let c = vertex(ring + 1, segment + 1);
            let d = vertex(ring + 1, segment);
            if ring > 0 {
                out.push([a, b, c]);
            }
            if ring < 7 {
                out.push([a, c, d]);
            }
        }
    }
    Ok(())
}
pub fn decode_col(data: &[u8]) -> Result<Vec<CollisionModel>> {
    ensure!(
        data.len() <= 16 * 1024 * 1024,
        "COL container exceeds budget"
    );
    let mut models = Vec::new();
    let mut cursor = 0;
    while cursor < data.len() {
        if data[cursor..].iter().all(|b| *b == 0) {
            break;
        }
        let len = (u32at(data, cursor + 4)? as usize)
            .checked_add(8)
            .context("COL size overflow")?;
        ensure!(len >= 72, "invalid COL size");
        let d = slice(data, cursor, len)?;
        let model_name = name(slice(d, 8, 22)?)?;
        let mut triangles = Vec::new();
        let (min, max);
        if &d[..4] == b"COLL" {
            min = vector(d, 48)?;
            max = vector(d, 60)?;
            let mut p = 72;
            let spheres = u32at(d, p)? as usize;
            p += 4;
            ensure!(spheres <= 65535, "COL sphere budget");
            slice(d, p, spheres * 20)?;
            for _ in 0..spheres {
                add_sphere(&mut triangles, vector(d, p + 4)?, f32at(d, p)?)?;
                p += 20;
            }
            let lines = u32at(d, p)? as usize;
            p += 4;
            ensure!(lines == 0, "unsupported COL1 lines");
            let boxes = u32at(d, p)? as usize;
            p += 4;
            ensure!(boxes <= 65535, "COL box budget");
            slice(d, p, boxes * 28)?;
            for _ in 0..boxes {
                add_box(&mut triangles, vector(d, p)?, vector(d, p + 12)?)?;
                p += 28;
            }
            let vertices = u32at(d, p)? as usize;
            p += 4;
            ensure!(vertices <= 65536, "COL vertex budget");
            let points: Vec<_> = (0..vertices)
                .map(|i| vector(d, p + i * 12))
                .collect::<Result<_>>()?;
            p += vertices * 12;
            let faces = u32at(d, p)? as usize;
            p += 4;
            ensure!(faces <= 65535, "COL face budget");
            slice(d, p, faces * 16)?;
            for row in d[p..p + faces * 16].as_chunks::<16>().0.iter() {
                let mut face = [[0.0; 3]; 3];
                for (a, v) in face.iter_mut().enumerate() {
                    *v = *points
                        .get(u32at(row, a * 4)? as usize)
                        .context("COL vertex index")?;
                }
                triangles.push(face);
            }
        } else {
            ensure!(
                &d[..4] == b"COL2" || &d[..4] == b"COL3" || &d[..4] == b"COL4",
                "unsupported COL version"
            );
            slice(
                d,
                0,
                if &d[..4] == b"COL2" {
                    108
                } else if &d[..4] == b"COL3" {
                    120
                } else {
                    124
                },
            )?;
            min = vector(d, 32)?;
            max = vector(d, 44)?;
            let offset = |p| -> Result<usize> { Ok(u32at(d, p)? as usize + 4) };
            let spheres = u16at(d, 72)? as usize;
            let boxes = u16at(d, 74)? as usize;
            let faces = u16at(d, 76)? as usize;
            ensure!(
                spheres * 224 + boxes * 12 + faces <= 2_000_000,
                "COL primitive budget"
            );
            if spheres > 0 {
                let p = offset(84)?;
                ensure!(p >= 108, "COL sphere offset");
                slice(d, p, spheres * 20)?;
                for i in 0..spheres {
                    add_sphere(
                        &mut triangles,
                        vector(d, p + i * 20)?,
                        f32at(d, p + i * 20 + 12)?,
                    )?;
                }
            }
            if boxes > 0 {
                let p = offset(88)?;
                ensure!(p >= 108, "COL box offset");
                slice(d, p, boxes * 28)?;
                for i in 0..boxes {
                    add_box(
                        &mut triangles,
                        vector(d, p + i * 28)?,
                        vector(d, p + i * 28 + 12)?,
                    )?;
                }
            }
            if faces > 0 {
                let vp = offset(96)?;
                let fp = offset(100)?;
                ensure!(vp >= 108 && fp >= 108, "COL mesh offsets");
                slice(d, fp, faces * 8)?;
                for row in d[fp..fp + faces * 8].as_chunks::<8>().0.iter() {
                    let mut face = [[0.0; 3]; 3];
                    for (axis, point) in face.iter_mut().enumerate() {
                        let p = vp + u16at(row, axis * 2)? as usize * 6;
                        let bytes = slice(d, p, 6)?;
                        for a in 0..3 {
                            point[a] = i16::from_le_bytes(bytes[a * 2..a * 2 + 2].try_into()?)
                                as f32
                                / 128.0;
                        }
                    }
                    triangles.push(face);
                }
            }
        }
        ensure!((0..3).all(|a| min[a] <= max[a]), "invalid COL bounds");
        ensure!(triangles.len() <= 2_000_000, "COL triangle budget");
        models.push(CollisionModel {
            name: model_name,
            triangles,
            min,
            max,
        });
        cursor += len;
    }
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn mesh() -> Vec<u8> {
        let mut d = vec![0; 146];
        d[..4].copy_from_slice(b"COL3");
        d[4..8].copy_from_slice(&138u32.to_le_bytes());
        d[8..12].copy_from_slice(b"road");
        for a in 0..3 {
            d[32 + a * 4..36 + a * 4].copy_from_slice(&(-1f32).to_le_bytes());
            d[44 + a * 4..48 + a * 4].copy_from_slice(&1f32.to_le_bytes());
        }
        d[76..78].copy_from_slice(&1u16.to_le_bytes());
        d[96..100].copy_from_slice(&116u32.to_le_bytes());
        d[100..104].copy_from_slice(&134u32.to_le_bytes());
        for (i, v) in [-128i16, 0, 0, 128, 0, 0, 0, 128, 0].iter().enumerate() {
            d[120 + i * 2..122 + i * 2].copy_from_slice(&v.to_le_bytes());
        }
        d[140..142].copy_from_slice(&1u16.to_le_bytes());
        d[142..144].copy_from_slice(&2u16.to_le_bytes());
        d
    }
    #[test]
    fn offsets_and_fixed_point() {
        let mut d = mesh();
        d.extend_from_slice(&mesh());
        d.extend([0; 20]);
        let models = decode_col(&d).unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].name, "road");
        assert_eq!(
            models[0].triangles[0],
            [[-1.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
        );
    }
    #[test]
    fn truncated_and_bad_offsets_are_rejected() {
        let d = mesh();
        assert!(decode_col(&d[..145]).is_err());
        let mut d = mesh();
        d[100..104].copy_from_slice(&200u32.to_le_bytes());
        assert!(decode_col(&d).is_err());
        let mut d = mesh();
        d[142..144].copy_from_slice(&65535u16.to_le_bytes());
        assert!(decode_col(&d).is_err());
    }
}
