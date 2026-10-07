//! Bounded, read-only decoders for the classic PC SA archive and rigid RenderWare assets.
pub mod col;
pub mod ifp;
pub mod skin;
use anyhow::{bail, ensure, Context, Result};
use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

fn slice(data: &[u8], start: usize, len: usize) -> Result<&[u8]> {
    data.get(start..start.checked_add(len).context("offset overflow")?)
        .context("truncated asset")
}
fn u16at(d: &[u8], p: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(slice(d, p, 2)?.try_into()?))
}
fn u32at(d: &[u8], p: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(slice(d, p, 4)?.try_into()?))
}
fn i32at(d: &[u8], p: usize) -> Result<i32> {
    Ok(i32::from_le_bytes(slice(d, p, 4)?.try_into()?))
}
fn f32at(d: &[u8], p: usize) -> Result<f32> {
    let n = f32::from_le_bytes(slice(d, p, 4)?.try_into()?);
    ensure!(n.is_finite(), "nonfinite asset float");
    Ok(n)
}
fn name(d: &[u8]) -> Result<String> {
    let b = d.split(|x| *x == 0).next().unwrap_or(d);
    ensure!(b.is_ascii(), "non-ASCII asset name");
    Ok(String::from_utf8(b.to_vec())?.to_ascii_lowercase())
}

#[derive(Clone, Copy)]
struct Entry {
    offset: u64,
    bytes: usize,
}
pub struct Img {
    file: File,
    entries: HashMap<String, Entry>,
}
impl Img {
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
        let length = file.metadata()?.len();
        let mut head = [0; 8];
        file.read_exact(&mut head)?;
        ensure!(&head[..4] == b"VER2", "unsupported IMG signature");
        let count = u32at(&head, 4)? as usize;
        ensure!(
            count <= 200_000 && 8 + count * 32 <= length as usize,
            "invalid IMG directory"
        );
        let mut directory = vec![0; count * 32];
        file.read_exact(&mut directory)?;
        let mut entries = HashMap::new();
        for row in directory.as_chunks::<32>().0.iter() {
            let sector = u32at(row, 0)? as u64;
            let streaming = u16at(row, 4)? as u64;
            let archived = u16at(row, 6)?;
            let key = name(&row[8..32])?;
            let offset = sector.checked_mul(2048).context("IMG offset overflow")?;
            let bytes = streaming.checked_mul(2048).context("IMG size overflow")?;
            // Unknown compressed variants are indexed, then rejected if requested.
            if archived == 0
                && streaming > 0
                && offset >= 8 + count as u64 * 32
                && offset + bytes <= length
            {
                ensure!(
                    entries
                        .insert(
                            key,
                            Entry {
                                offset,
                                bytes: bytes as usize
                            }
                        )
                        .is_none(),
                    "duplicate IMG entry"
                );
            }
        }
        Ok(Self { file, entries })
    }
    pub fn has(&self, name: &str) -> bool {
        self.entries.contains_key(&name.to_ascii_lowercase())
    }
    pub fn names(&self) -> impl Iterator<Item = &String> {
        self.entries.keys()
    }
    pub fn read(&mut self, name: &str) -> Result<Vec<u8>> {
        let entry = self
            .entries
            .get(&name.to_ascii_lowercase())
            .context("missing or unsupported IMG entry")?;
        ensure!(
            entry.bytes <= 16 * 1024 * 1024,
            "IMG entry exceeds 16 MiB limit"
        );
        self.file.seek(SeekFrom::Start(entry.offset))?;
        let mut data = vec![0; entry.bytes];
        self.file.read_exact(&mut data)?;
        Ok(data)
    }
}

#[derive(Clone, Copy)]
struct Chunk<'a> {
    tag: u32,
    body: &'a [u8],
}
fn chunks(mut data: &[u8]) -> Result<Vec<Chunk<'_>>> {
    let mut out = Vec::new();
    while !data.is_empty() {
        ensure!(data.len() >= 12, "truncated RenderWare chunk");
        let tag = u32at(data, 0)?;
        let size = u32at(data, 4)? as usize;
        let end = 12usize.checked_add(size).context("chunk size overflow")?;
        let body = slice(data, 12, size)?;
        out.push(Chunk { tag, body });
        data = &data[end..];
    }
    Ok(out)
}
fn one(data: &[u8], tag: u32) -> Result<&[u8]> {
    let found: Vec<_> = chunks(data)?.into_iter().filter(|c| c.tag == tag).collect();
    ensure!(found.len() == 1, "expected one RenderWare chunk {tag:#x}");
    Ok(found[0].body)
}
fn root(data: &[u8], tag: u32) -> Result<&[u8]> {
    ensure!(
        u32at(data, 0)? == tag && u32at(data, 8)? == 0x1803ffff,
        "unsupported RenderWare root/version"
    );
    slice(data, 12, u32at(data, 4)? as usize)
}
#[derive(Clone)]
pub struct Material {
    pub color: [u8; 4],
    pub texture: Option<String>,
}
#[derive(Clone)]
pub struct Geometry {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub colors: Vec<[u8; 4]>,
    pub triangles: Vec<[u16; 4]>,
    pub materials: Vec<Material>,
}
fn mat_list(data: &[u8]) -> Result<Vec<Material>> {
    let header = one(data, 1)?;
    let count = u32at(header, 0)? as usize;
    ensure!(
        (1..=256).contains(&count) && header.len() == 4 + count * 4,
        "invalid material list"
    );
    let fresh: Vec<_> = chunks(data)?.into_iter().filter(|c| c.tag == 7).collect();
    let mut cursor = 0;
    let mut materials: Vec<Material> = Vec::new();
    for i in 0..count {
        let reference = i32at(header, 4 + i * 4)?;
        if reference >= 0 {
            ensure!((reference as usize) < i, "invalid material reuse");
            materials.push(materials[reference as usize].clone());
            continue;
        }
        ensure!(reference == -1, "invalid material reference");
        let body = fresh.get(cursor).context("missing material")?.body;
        cursor += 1;
        let s = one(body, 1)?;
        ensure!(
            s.len() == 16 || s.len() == 28,
            "unsupported material structure"
        );
        let color: [u8; 4] = slice(s, 4, 4)?.try_into()?;
        let textured = u32at(s, 12)? != 0;
        let texture = if textured {
            let strings: Vec<_> = chunks(one(body, 6)?)?
                .into_iter()
                .filter(|c| c.tag == 2)
                .collect();
            ensure!(strings.len() == 2, "invalid material texture");
            Some(name(strings[0].body)?)
        } else {
            None
        };
        materials.push(Material { color, texture });
    }
    ensure!(cursor == fresh.len(), "extra material chunks");
    Ok(materials)
}

fn geometry(data: &[u8]) -> Result<Geometry> {
    let s = one(data, 1)?;
    let flags = u32at(s, 0)?;
    let nt = u32at(s, 4)? as usize;
    let nv = u32at(s, 8)? as usize;
    ensure!(
        flags & 0x0100_0000 == 0 && u32at(s, 12)? == 1,
        "native or multi-morph geometry unsupported"
    );
    ensure!(
        (1..=65_535).contains(&nv) && (1..=200_000).contains(&nt),
        "geometry count outside bounds"
    );
    let mut uv_count = ((flags >> 16) & 255) as usize;
    if uv_count == 0 {
        uv_count = if flags & 0x80 != 0 {
            2
        } else if flags & 4 != 0 {
            1
        } else {
            0
        };
    }
    ensure!(uv_count <= 8, "excess UV sets");
    let mut p = 16;
    let mut colors = vec![[255; 4]; nv];
    if flags & 8 != 0 {
        for color in &mut colors {
            *color = slice(s, p, 4)?.try_into()?;
            p += 4;
        }
    }
    let mut uvs = vec![[0.0; 2]; nv];
    for layer in 0..uv_count {
        for uv in &mut uvs {
            // A few shipped meshes contain NaN UVs despite valid positions.
            // Use the texture origin for those coordinates; bounds and all
            // spatial data remain strictly checked.
            let coordinate = |offset| -> Result<f32> {
                let value = f32::from_le_bytes(slice(s, offset, 4)?.try_into()?);
                Ok(if value.is_finite() { value } else { 0.0 })
            };
            let value = [coordinate(p)?, coordinate(p + 4)?];
            p += 8;
            if layer == 0 {
                *uv = value;
            }
        }
    }
    let mut triangles = Vec::with_capacity(nt);
    for _ in 0..nt {
        let b = u16at(s, p)?;
        let a = u16at(s, p + 2)?;
        let m = u16at(s, p + 4)?;
        let c = u16at(s, p + 6)?;
        p += 8;
        ensure!(usize::from(a.max(b).max(c)) < nv, "invalid triangle index");
        triangles.push([a, b, c, m]);
    }
    for i in 0..4 {
        f32at(s, p + i * 4)?;
    }
    p += 16;
    ensure!(u32at(s, p)? != 0, "missing positions");
    let has_normals = u32at(s, p + 4)? != 0;
    p += 8;
    let mut positions = Vec::with_capacity(nv);
    for _ in 0..nv {
        positions.push([f32at(s, p)?, f32at(s, p + 4)?, f32at(s, p + 8)?]);
        p += 12;
    }
    let mut normals = Vec::new();
    if has_normals {
        for _ in 0..nv {
            normals.push([f32at(s, p)?, f32at(s, p + 4)?, f32at(s, p + 8)?]);
            p += 12;
        }
    }
    ensure!(p == s.len(), "unsupported geometry tail");
    let materials = mat_list(one(data, 8)?)?;
    ensure!(
        triangles
            .iter()
            .all(|t| usize::from(t[3]) < materials.len()),
        "invalid triangle material"
    );
    Ok(Geometry {
        positions,
        normals,
        uvs,
        colors,
        triangles,
        materials,
    })
}
fn transform(m: &[f32; 12], v: [f32; 3], translation: bool) -> [f32; 3] {
    let offset = if translation {
        [m[9], m[10], m[11]]
    } else {
        [0.0; 3]
    };
    [
        m[0] * v[0] + m[3] * v[1] + m[6] * v[2] + offset[0],
        m[1] * v[0] + m[4] * v[1] + m[7] * v[2] + offset[1],
        m[2] * v[0] + m[5] * v[1] + m[8] * v[2] + offset[2],
    ]
}
fn compose(a: &[f32; 12], b: &[f32; 12]) -> [f32; 12] {
    let mut out = [0.0; 12];
    for axis in 0..3 {
        let v = transform(a, [b[axis * 3], b[axis * 3 + 1], b[axis * 3 + 2]], false);
        out[axis * 3..axis * 3 + 3].copy_from_slice(&v);
    }
    out[9..12].copy_from_slice(&transform(a, [b[9], b[10], b[11]], true));
    out
}
pub fn decode_dff(data: &[u8]) -> Result<Vec<Geometry>> {
    decode_dff_parts(data, false)
}
pub fn decode_vehicle_dff(data: &[u8]) -> Result<Vec<Geometry>> {
    decode_dff_parts(data, true)
}
fn decode_dff_parts(data: &[u8], vehicle: bool) -> Result<Vec<Geometry>> {
    // Some original SA models serialize a UV animation dictionary before
    // the clump. Its presence must not hide otherwise supported geometry.
    let mut stream = data;
    let mut dictionaries = 0;
    while u32at(stream, 0)? == 43 {
        ensure!(dictionaries < 16, "DFF dictionary budget");
        let dictionary = root(stream, 43)?;
        let sections = chunks(dictionary)?;
        ensure!(
            sections.first().is_some_and(|c| c.tag == 1),
            "missing UV dictionary structure"
        );
        stream = &stream[12 + dictionary.len()..];
        dictionaries += 1;
    }
    let clump = root(stream, 16)?;
    let sections = chunks(clump)?;
    ensure!(
        sections.first().is_some_and(|c| c.tag == 1),
        "missing clump structure"
    );
    let cs = sections[0].body;
    let na = u32at(cs, 0)? as usize;
    ensure!(
        (1..=128).contains(&na) && u32at(cs, 8)? == 0,
        "unsupported clump"
    );
    let fs = one(one(clump, 14)?, 1)?;
    let nf = u32at(fs, 0)? as usize;
    ensure!(
        (1..=256).contains(&nf) && fs.len() == 4 + nf * 56,
        "invalid frame list"
    );
    let mut local = Vec::new();
    let mut parents = Vec::new();
    for i in 0..nf {
        let p = 4 + i * 56;
        let mut m = [0.0; 12];
        for (j, v) in m.iter_mut().enumerate() {
            *v = f32at(fs, p + j * 4)?;
        }
        let parent = i32at(fs, p + 48)?;
        ensure!(
            parent >= -1 && (parent as i64) < nf as i64 && parent != i as i32,
            "invalid frame parent"
        );
        local.push(m);
        parents.push(parent);
    }
    fn frame(
        i: usize,
        local: &[[f32; 12]],
        parents: &[i32],
        states: &mut [u8],
        out: &mut [[f32; 12]],
    ) -> Result<()> {
        ensure!(states[i] != 1, "cyclic frame hierarchy");
        if states[i] == 2 {
            return Ok(());
        }
        states[i] = 1;
        out[i] = if parents[i] < 0 {
            local[i]
        } else {
            let parent = parents[i] as usize;
            frame(parent, local, parents, states, out)?;
            compose(&out[parent], &local[i])
        };
        states[i] = 2;
        Ok(())
    }
    let mut frames = vec![[0.0; 12]; nf];
    let mut states = vec![0; nf];
    for i in 0..nf {
        frame(i, &local, &parents, &mut states, &mut frames)?;
    }
    let gl = one(clump, 26)?;
    let ng = u32at(one(gl, 1)?, 0)? as usize;
    let geometries: Vec<_> = chunks(gl)?
        .into_iter()
        .filter(|c| c.tag == 15)
        .map(|c| geometry(c.body))
        .collect::<Result<_>>()?;
    ensure!(
        (1..=128).contains(&ng) && ng == geometries.len(),
        "invalid geometry list"
    );
    let atomics: Vec<_> = chunks(clump)?.into_iter().filter(|c| c.tag == 20).collect();
    ensure!(atomics.len() == na, "invalid atomic count");
    let mut out = Vec::new();
    let frame_names: Vec<String> = chunks(one(clump, 14)?)?
        .into_iter()
        .filter(|c| c.tag == 3)
        .map(|c| {
            Ok(chunks(c.body)?
                .into_iter()
                .find(|c| c.tag == 0x0253f2fe)
                .map(|c| {
                    String::from_utf8_lossy(c.body)
                        .trim_end_matches('\0')
                        .to_ascii_lowercase()
                })
                .unwrap_or_default())
        })
        .collect::<Result<_>>()?;
    ensure!(frame_names.len() <= nf, "excess frame extensions");
    for atomic in atomics {
        let s = one(atomic.body, 1)?;
        let fi = u32at(s, 0)? as usize;
        let gi = u32at(s, 4)? as usize;
        ensure!(fi < nf && gi < ng, "invalid atomic references");
        let frame_name = frame_names.get(fi).map(String::as_str).unwrap_or("");
        if vehicle
            && (frame_name.ends_with("_dam")
                || frame_name.ends_with("_vlo")
                || frame_name.starts_with("extra"))
        {
            continue;
        }
        if vehicle
            && frame_name == "wheel"
            && frame_names
                .iter()
                .any(|n| n.starts_with("wheel_") && n.ends_with("_dummy"))
        {
            for (index, name) in frame_names.iter().enumerate() {
                if name.starts_with("wheel_") && name.ends_with("_dummy") {
                    let matrix = compose(&frames[index], &local[fi]);
                    let mut wheel = geometries[gi].clone();
                    for vertex in &mut wheel.positions {
                        *vertex = transform(&matrix, *vertex, true);
                    }
                    out.push(wheel);
                }
            }
            continue;
        }
        let mut g = geometries[gi].clone();
        for v in &mut g.positions {
            *v = transform(&frames[fi], *v, true);
        }
        for v in &mut g.normals {
            *v = transform(&frames[fi], *v, false);
        }
        out.push(g);
    }
    Ok(out)
}

#[derive(Clone)]
pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub has_alpha: bool,
}
impl Texture {
    /// Binary alpha and narrow antialiased edges use depth-writing cutouts.
    /// Broad partial opacity (glass, smoke, shadows) needs a blended pass.
    pub fn needs_blending(&self) -> bool {
        self.has_alpha
            && self
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[3] > 8 && p[3] < 247)
                .count()
                * 10
                > self.rgba.len() / 4
    }
}
fn rgb565(c: u16) -> [u8; 3] {
    [
        ((c >> 11) as u32 * 255 / 31) as u8,
        (((c >> 5) & 63) as u32 * 255 / 63) as u8,
        ((c & 31) as u32 * 255 / 31) as u8,
    ]
}
fn blocks(data: &[u8], w: usize, h: usize, kind: u32, alpha: bool) -> Result<Vec<u8>> {
    let stride = if kind == u32::from_le_bytes(*b"DXT1") {
        8
    } else {
        16
    };
    ensure!(
        data.len() == w.div_ceil(4) * h.div_ceil(4) * stride,
        "invalid compressed mip length"
    );
    let mut rgba = vec![0; w * h * 4];
    let mut p = 0;
    for by in (0..h).step_by(4) {
        for bx in (0..w).step_by(4) {
            let mut a = [255u8; 16];
            if kind == u32::from_le_bytes(*b"DXT3") {
                let bits = u64::from_le_bytes(slice(data, p, 8)?.try_into()?);
                p += 8;
                for (i, v) in a.iter_mut().enumerate() {
                    *v = (((bits >> (i * 4)) & 15) * 17) as u8;
                }
            }
            if kind == u32::from_le_bytes(*b"DXT5") {
                let x = data[p] as u32;
                let y = data[p + 1] as u32;
                let mut selector = 0u64;
                for i in 0..6 {
                    selector |= (data[p + 2 + i] as u64) << (8 * i);
                }
                p += 8;
                let mut values = [0u8; 8];
                values[0] = x as u8;
                values[1] = y as u8;
                if x > y {
                    for i in 1..7u32 {
                        values[(i + 1) as usize] = (((7 - i) * x + i * y) / 7) as u8;
                    }
                } else {
                    for i in 1..5u32 {
                        values[(i + 1) as usize] = (((5 - i) * x + i * y) / 5) as u8;
                    }
                    values[6] = 0;
                    values[7] = 255;
                }
                for (i, v) in a.iter_mut().enumerate() {
                    *v = values[((selector >> (3 * i)) & 7) as usize];
                }
            }
            let c0 = u16at(data, p)?;
            let c1 = u16at(data, p + 2)?;
            let bits = u32at(data, p + 4)?;
            p += 8;
            let mut pal = [[0u8; 4]; 4];
            for (dst, c) in [(0, c0), (1, c1)] {
                pal[dst][..3].copy_from_slice(&rgb565(c));
                pal[dst][3] = 255;
            }
            let color0 = pal[0];
            let color1 = pal[1];
            for (ch, (a, b)) in color0[..3].iter().zip(&color1[..3]).enumerate() {
                if kind != u32::from_le_bytes(*b"DXT1") || c0 > c1 {
                    pal[2][ch] = ((2 * *a as u32 + *b as u32) / 3) as u8;
                    pal[3][ch] = ((*a as u32 + 2 * *b as u32) / 3) as u8;
                } else {
                    pal[2][ch] = ((*a as u32 + *b as u32) / 2) as u8;
                }
            }
            pal[2][3] = 255;
            pal[3][3] = if kind == u32::from_le_bytes(*b"DXT1") && c0 <= c1 && alpha {
                0
            } else {
                255
            };
            for i in 0..16 {
                let x = bx + i % 4;
                let y = by + i / 4;
                if x >= w || y >= h {
                    continue;
                }
                let mut color = pal[((bits >> (2 * i)) & 3) as usize];
                if kind != u32::from_le_bytes(*b"DXT1") {
                    color[3] = a[i];
                }
                rgba[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&color);
            }
        }
    }
    Ok(rgba)
}
pub fn decode_txd(data: &[u8], wanted: &str) -> Result<Texture> {
    let dictionary = root(data, 22)?;
    let count = u16at(one(dictionary, 1)?, 0)? as usize;
    ensure!((1..=256).contains(&count), "invalid TXD count");
    let natives: Vec<_> = chunks(dictionary)?
        .into_iter()
        .filter(|c| c.tag == 21)
        .collect();
    ensure!(natives.len() == count, "TXD count mismatch");
    for native in natives {
        let s = one(native.body, 1)?;
        let key = name(slice(s, 8, 32)?)?;
        if key != wanted.to_ascii_lowercase() {
            continue;
        }
        ensure!(s.len() >= 88, "truncated native texture header");
        ensure!(u32at(s, 0)? == 9, "unsupported texture platform");
        let raster = u32at(s, 72)?;
        let format = u32at(s, 76)?;
        let w = u16at(s, 80)? as usize;
        let h = u16at(s, 82)? as usize;
        let depth = s[84];
        let levels = s[85] as usize;
        let props = s[87];
        ensure!(
            (1..=4096).contains(&w)
                && (1..=4096).contains(&h)
                && (1..=13).contains(&levels)
                && raster & 0x6000 == 0
                && props & 6 == 0,
            "unsupported texture layout"
        );
        let dxt1 = u32::from_le_bytes(*b"DXT1");
        let dxt3 = u32::from_le_bytes(*b"DXT3");
        let dxt5 = u32::from_le_bytes(*b"DXT5");
        ensure!(
            [dxt1, dxt3, dxt5, 21, 22].contains(&format),
            "unsupported texture compression"
        );
        if format == 21 || format == 22 {
            ensure!(depth == 32, "unsupported BGRA depth");
        }
        let mut p = 88;
        let mut rgba = Vec::new();
        let mut empty_tail = false;
        for level in 0..levels {
            let mw = (w >> level).max(1);
            let mh = (h >> level).max(1);
            let size = u32at(s, p)? as usize;
            p += 4;
            let expected = if format == 21 || format == 22 {
                mw * mh * 4
            } else {
                mw.div_ceil(4) * mh.div_ceil(4) * if format == dxt1 { 8 } else { 16 }
            };
            // Some original SA DXT textures declare empty final mips below a
            // full 4x4 block. Their earlier levels remain usable.
            if size == 0 {
                ensure!(
                    level > 0 && format != 21 && format != 22 && (mw < 4 || mh < 4),
                    "invalid empty native mip"
                );
                empty_tail = true;
                continue;
            }
            ensure!(!empty_tail, "nonempty mip after empty tail");
            ensure!(size == expected, "invalid native mip size");
            let pixels = slice(s, p, size)?;
            p += size;
            if level == 0 {
                rgba = if format == 21 || format == 22 {
                    pixels
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .flat_map(|v| [v[2], v[1], v[0], if format == 21 { v[3] } else { 255 }])
                        .collect()
                } else {
                    blocks(pixels, mw, mh, format, props & 1 != 0)?
                };
            }
        }
        ensure!(p == s.len(), "invalid native texture tail");
        let has_alpha = rgba.as_chunks::<4>().0.iter().any(|v| v[3] != 255);
        return Ok(Texture {
            width: w as u32,
            height: h as u32,
            rgba,
            has_alpha,
        });
    }
    bail!("material texture {wanted} absent from TXD")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vehicle_variant_filter_keeps_intact_parts() {
        fn chunk(tag: u32, body: &[u8]) -> Vec<u8> {
            let mut bytes = Vec::new();
            bytes.extend(tag.to_le_bytes());
            bytes.extend((body.len() as u32).to_le_bytes());
            bytes.extend(0x1803ffffu32.to_le_bytes());
            bytes.extend(body);
            bytes
        }
        let room = include_bytes!("../../../../mods/native-room-demo/room.dff");
        for (name, visible) in [
            ("chassis", true),
            ("door_ok", true),
            ("door_dam", false),
            ("chassis_vlo", false),
            ("extra1", false),
        ] {
            let mut body = Vec::new();
            for section in chunks(root(room, 16).unwrap()).unwrap() {
                if section.tag == 14 {
                    let mut frames = Vec::new();
                    for frame in chunks(section.body).unwrap() {
                        if frame.tag == 3 {
                            frames.extend(chunk(3, &chunk(0x0253f2fe, name.as_bytes())));
                        } else {
                            frames.extend(chunk(frame.tag, frame.body));
                        }
                    }
                    body.extend(chunk(14, &frames));
                } else {
                    body.extend(chunk(section.tag, section.body));
                }
            }
            let fixture = chunk(16, &body);
            assert_eq!(decode_dff(&fixture).unwrap().len(), 1);
            assert_eq!(
                decode_vehicle_dff(&fixture).unwrap().len(),
                usize::from(visible)
            );
        }
    }
    #[test]
    fn invalid_uvs_have_fallback_but_invalid_positions_are_rejected() {
        let room = include_bytes!("../../../../mods/native-room-demo/room.dff");
        let list = one(root(room, 16).unwrap(), 26).unwrap();
        let geometry = one(list, 15).unwrap();
        let s = one(geometry, 1).unwrap();
        let start = s.as_ptr() as usize - room.as_ptr() as usize;
        let flags = u32at(s, 0).unwrap();
        let nv = u32at(s, 8).unwrap() as usize;
        let nt = u32at(s, 4).unwrap() as usize;
        let uv_start = start + 16 + if flags & 8 != 0 { nv * 4 } else { 0 };
        let mut invalid = room.to_vec();
        // The generated room has no UV layer; add one to this test fixture.
        assert_eq!(flags & 4, 0);
        invalid.splice(uv_start..uv_start, vec![0u8; nv * 8]);
        invalid[start..start + 4].copy_from_slice(&(flags | 4).to_le_bytes());
        for body in [root(room, 16).unwrap(), list, geometry, s] {
            let header = body.as_ptr() as usize - room.as_ptr() as usize - 12;
            let length = u32at(room, header + 4).unwrap() + (nv * 8) as u32;
            invalid[header + 4..header + 8].copy_from_slice(&length.to_le_bytes());
        }
        invalid[uv_start..uv_start + 4].copy_from_slice(&f32::NAN.to_le_bytes());
        invalid[uv_start + 4..uv_start + 8].copy_from_slice(&f32::INFINITY.to_le_bytes());
        assert_eq!(decode_dff(&invalid).unwrap()[0].uvs[0], [0.0, 0.0]);
        let position_start = uv_start + nv * 8 + nt * 8 + 24;
        invalid[position_start..position_start + 4].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(decode_dff(&invalid).is_err());
    }
    #[test]
    fn uv_dictionary_prefix_preserves_geometry_and_checks_bounds() {
        let room = include_bytes!("../../../../mods/native-room-demo/room.dff");
        let base = decode_dff(room).unwrap();
        let mut prefix = Vec::new();
        for word in [43u32, 16, 0x1803ffff, 1, 4, 0x1803ffff, 0] {
            prefix.extend(word.to_le_bytes());
        }
        let mut stream = prefix.clone();
        stream.extend(room);
        let decoded = decode_dff(&stream).unwrap();
        assert_eq!(decoded.len(), base.len());
        assert_eq!(decoded[0].positions, base[0].positions);
        assert_eq!(decoded[0].triangles, base[0].triangles);
        assert!(decode_dff(&prefix).is_err());
        stream[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode_dff(&stream).is_err());
        stream[4..8].copy_from_slice(&16u32.to_le_bytes());
        stream[8..12].copy_from_slice(&0u32.to_le_bytes());
        assert!(decode_dff(&stream).is_err());
    }
    #[test]
    fn img_rejects_bad_directory_and_out_of_range_entries() {
        let path = std::env::temp_dir().join(format!("sa-img-test-{}", std::process::id()));
        std::fs::write(&path, b"VER2\x01\x00\x00\x00").unwrap();
        assert!(Img::open(&path).is_err());
        let mut data = vec![0u8; 2048];
        data[..8].copy_from_slice(b"VER2\x01\x00\x00\x00");
        data[8..12].copy_from_slice(&99u32.to_le_bytes());
        data[12..14].copy_from_slice(&1u16.to_le_bytes());
        data[16..24].copy_from_slice(b"bad.dff\0");
        std::fs::write(&path, &data).unwrap();
        assert!(!Img::open(&path).unwrap().has("bad.dff"));
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn compressed_block_length_is_checked() {
        assert!(blocks(&[0; 7], 4, 4, u32::from_le_bytes(*b"DXT1"), true).is_err());
    }
    #[test]
    fn original_style_empty_dxt_mip_tail() {
        fn chunk(tag: u32, body: &[u8]) -> Vec<u8> {
            let mut out = Vec::new();
            out.extend(tag.to_le_bytes());
            out.extend((body.len() as u32).to_le_bytes());
            out.extend(0x1803ffffu32.to_le_bytes());
            out.extend(body);
            out
        }
        let mut native = vec![0u8; 88];
        native[0..4].copy_from_slice(&9u32.to_le_bytes());
        native[8..13].copy_from_slice(b"test\0");
        native[76..80].copy_from_slice(b"DXT1");
        native[80..82].copy_from_slice(&8u16.to_le_bytes());
        native[82..84].copy_from_slice(&8u16.to_le_bytes());
        native[84] = 16;
        native[85] = 4;
        for size in [32u32, 8, 0, 0] {
            native.extend(size.to_le_bytes());
            native.extend(vec![0; size as usize]);
        }
        let mut dictionary = chunk(1, &[1, 0, 0, 0]);
        dictionary.extend(chunk(21, &chunk(1, &native)));
        let texture = decode_txd(&chunk(22, &dictionary), "test").unwrap();
        assert_eq!(
            (texture.width, texture.height, texture.rgba.len()),
            (8, 8, 256)
        );
    }
}
