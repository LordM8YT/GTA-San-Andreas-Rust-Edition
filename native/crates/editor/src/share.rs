//! Share a world: pack the saved world folder into one ZIP that anyone can
//! install, with its size and SHA-256 and the ready-made entry for
//! `community/worlds.json`, so it shows in SARE Client's Discover.
//!
//! The ZIP holds the folder itself (`<id>/content.json`, ...), which is what
//! Discover installs. Hidden files (the autosave, catalog markers) stay out.
use anyhow::{bail, ensure, Context, Result};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};

/// Licence a new world starts with: nobody may share it until its maker
/// picks another.
pub const DEFAULT_LICENSE: &str = "All rights reserved";

/// Licences offered in the world settings: (SPDX id or text, what it means).
pub const LICENSES: &[(&str, &str)] = &[
    (
        "CC0-1.0",
        "Public domain: anyone may use, change and share it, no credit needed",
    ),
    (
        "CC-BY-4.0",
        "Anyone may use, change and share it if they credit you",
    ),
    (
        "CC-BY-SA-4.0",
        "Like CC-BY, and changed copies must use the same licence",
    ),
    ("CC-BY-NC-4.0", "Like CC-BY, but not for money"),
    ("MIT", "Open source: use, change and share, keep the notice"),
    (
        DEFAULT_LICENSE,
        "Only you may share it; players may still play it",
    ),
];

/// Discover's limits (the client refuses bigger worlds).
pub const MAX_FILES: usize = 1024;
pub const MAX_BYTES: u64 = 256 * 1024 * 1024;

/// What an export made.
#[derive(Debug)]
pub struct Exported {
    pub zip: PathBuf,
    pub bytes: u64,
    pub sha256: String,
    pub files: usize,
    /// The world's entry for `community/worlds.json`, with the download
    /// link still to fill in.
    pub entry: String,
    /// Written beside the ZIP: the entry and how to share.
    pub notes: PathBuf,
}

/// Placeholder for the link the maker gets after uploading the ZIP.
pub const DOWNLOAD_PLACEHOLDER: &str = "https://PUT-THE-LINK-TO-YOUR-ZIP-HERE";

/// Where exports go for a world in `content_dir`: `exports/` beside it.
pub fn exports_dir(content_dir: &Path) -> PathBuf {
    content_dir
        .parent()
        .map(|p| p.join("exports"))
        .unwrap_or_else(|| PathBuf::from("exports"))
}

/// Pack the saved world in `folder` into `<out_dir>/<id>-<version>.zip`.
pub fn export(folder: &Path, out_dir: &Path) -> Result<Exported> {
    let manifest = sa_scene::content_pack::Manifest::read(folder)
        .context("save the world first: its content.json does not open")?;
    let mut files = Vec::new();
    collect(folder, "", &mut files)?;
    ensure!(
        files.len() <= MAX_FILES,
        "the world has {} files; Discover takes at most {MAX_FILES}",
        files.len()
    );
    let total: u64 = files
        .iter()
        .map(|(_, path)| std::fs::metadata(path).map(|m| m.len()).unwrap_or(0))
        .sum();
    ensure!(
        total <= MAX_BYTES,
        "the world is {} MiB; Discover takes at most {} MiB",
        total / (1024 * 1024),
        MAX_BYTES / (1024 * 1024)
    );
    std::fs::create_dir_all(out_dir).with_context(|| format!("creating {}", out_dir.display()))?;
    let stem = format!(
        "{}-{}",
        manifest.id,
        crate::project::sanitize(&manifest.version)
    );
    let zip_path = out_dir.join(format!("{stem}.zip"));
    let partial = out_dir.join(format!(".{stem}.zip.part"));
    {
        let file = std::fs::File::create(&partial)
            .with_context(|| format!("creating {}", partial.display()))?;
        let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(file));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o644);
        for (relative, path) in &files {
            zip.start_file(format!("{}/{relative}", manifest.id), options)?;
            let bytes =
                std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
            zip.write_all(&bytes)?;
        }
        zip.finish()?.flush()?;
    }
    std::fs::rename(&partial, &zip_path)?;
    let data = std::fs::read(&zip_path)?;
    let sha256 = hex(&Sha256::digest(&data));
    let bytes = data.len() as u64;
    let entry = entry(&manifest, &sha256, bytes);
    let notes = out_dir.join(format!("{stem}.share.txt"));
    std::fs::write(&notes, notes_text(&manifest, &zip_path, &entry))?;
    Ok(Exported {
        zip: zip_path,
        bytes,
        sha256,
        files: files.len(),
        entry,
        notes,
    })
}

/// A world's card picture in SARE Client (16:9).
pub const COVER: &str = "cover.png";
/// Covers are at most this wide (720 high).
pub const COVER_WIDTH: u32 = 1280;

/// A cover from a screenshot: the middle 16:9 of `rgba` (`width` x `height`,
/// tightly packed RGBA), scaled down to at most 1280 x 720 with a box filter,
/// as PNG bytes. Fully opaque, since the client draws it as a card.
pub fn cover_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        width >= 16 && height >= 9 && rgba.len() == width as usize * height as usize * 4,
        "the screenshot is empty"
    );
    // The largest 16:9 rectangle in the middle.
    let (crop_w, crop_h) = if width * 9 >= height * 16 {
        (height * 16 / 9, height)
    } else {
        (width, width * 9 / 16)
    };
    let (x0, y0) = ((width - crop_w) / 2, (height - crop_h) / 2);
    let out_w = crop_w.min(COVER_WIDTH);
    let out_h = (out_w * 9 / 16).max(1);
    let mut out = Vec::with_capacity(out_w as usize * out_h as usize * 4);
    for oy in 0..out_h {
        let sy0 = y0 + oy * crop_h / out_h;
        let sy1 = (y0 + (oy + 1) * crop_h / out_h).max(sy0 + 1);
        for ox in 0..out_w {
            let sx0 = x0 + ox * crop_w / out_w;
            let sx1 = (x0 + (ox + 1) * crop_w / out_w).max(sx0 + 1);
            let mut sum = [0u32; 3];
            let mut count = 0;
            for sy in sy0..sy1 {
                let row = sy as usize * width as usize;
                for sx in sx0..sx1 {
                    let i = (row + sx as usize) * 4;
                    for c in 0..3 {
                        sum[c] += u32::from(rgba[i + c]);
                    }
                    count += 1;
                }
            }
            out.extend(sum.map(|v| (v / count) as u8));
            out.push(255);
        }
    }
    let mut png_bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png_bytes, out_w, out_h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(&out)?;
    }
    Ok(png_bytes)
}

/// Every file to share, as (path inside the world with `/`, path on disk),
/// sorted so the same world always makes the same ZIP listing.
fn collect(dir: &Path, prefix: &str, out: &mut Vec<(String, PathBuf)>) -> Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .collect::<std::io::Result<_>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            bail!(
                "{} has a name that is not plain text; rename it",
                entry.path().display()
            );
        };
        if name.starts_with('.') {
            continue;
        }
        let kind = entry.file_type()?;
        let relative = format!("{prefix}{name}");
        if kind.is_dir() {
            collect(&entry.path(), &format!("{relative}/"), out)?;
        } else if kind.is_file() {
            out.push((relative, entry.path()));
        }
        // Links are left out: they could point anywhere on this PC.
    }
    Ok(())
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn entry(manifest: &sa_scene::content_pack::Manifest, sha256: &str, bytes: u64) -> String {
    let short = |text: &str, max: usize| text.chars().take(max).collect::<String>();
    let value = serde_json::json!({
        "id": manifest.id,
        "name": short(&manifest.name, 48),
        "author": short(manifest.author.as_deref().unwrap_or(""), 48),
        "description": short(manifest.description.as_deref().unwrap_or(""), 300),
        "version": short(&manifest.version, 24),
        "license": short(&manifest.license, 64),
        "download": DOWNLOAD_PLACEHOLDER,
        "sha256": sha256,
        "bytes": bytes,
    });
    serde_json::to_string_pretty(&value).expect("a JSON value prints")
}

fn notes_text(manifest: &sa_scene::content_pack::Manifest, zip: &Path, entry: &str) -> String {
    let file = zip.file_name().unwrap_or_default().to_string_lossy();
    let mut text = format!(
        "{name} is ready to share: {file}\n\n\
         Send the ZIP to a friend: they unpack it into SARE's content folder\n\
         (or install it from Discover once it is listed).\n\n\
         To list it in Discover for everyone:\n\
         1. Upload {file} somewhere that serves it over https:// without a login,\n   \
         for example as a file on a GitHub release of your own repository.\n\
         2. Put that link in \"download\" below.\n\
         3. Open a pull request on the SARE repository that adds this entry to\n   \
         community/worlds.json (see community/README.md).\n\n{entry}\n",
        name = manifest.name
    );
    if manifest.license.trim() == DEFAULT_LICENSE {
        text.push_str(
            "\nNote: the licence is \"All rights reserved\". Pick a licence in Studio\n\
             (Workspace > Sharing) if others may copy or change your world.\n",
        );
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::WorldInfo;
    use crate::project::Project;

    #[test]
    fn exports_the_saved_world_as_an_installable_zip() {
        let root = std::env::temp_dir().join(format!("sare-share-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let folder = root.join("content/hill");
        let (project, mut doc) = Project::create(&folder, "hill", "Hill").unwrap();
        doc.info = WorldInfo {
            author: "Kim".into(),
            version: "1.2.0".into(),
            license: "CC-BY-4.0".into(),
            ..doc.info.clone()
        };
        project.save(&doc).unwrap();
        let (_, reopened) = Project::open(&folder).unwrap();
        assert_eq!(
            reopened.info, doc.info,
            "author, version and licence survive a save"
        );
        std::fs::write(folder.join(".autosave.json"), "{}").unwrap();
        std::fs::create_dir_all(folder.join("assets/custom")).unwrap();
        std::fs::write(folder.join("assets/custom/rock.glb"), b"glb").unwrap();

        let out = exports_dir(&root.join("content"));
        let done = export(&folder, &out).unwrap();
        assert_eq!(done.zip, root.join("exports/hill-1-2-0.zip"));
        assert_eq!(done.bytes, std::fs::metadata(&done.zip).unwrap().len());
        assert_eq!(done.sha256.len(), 64);

        let mut archive = zip::ZipArchive::new(std::fs::File::open(&done.zip).unwrap()).unwrap();
        let names: Vec<String> = archive.file_names().map(str::to_owned).collect();
        assert!(names.contains(&"hill/content.json".to_owned()));
        assert!(names.contains(&"hill/layout.json".to_owned()));
        assert!(names.contains(&"hill/assets/custom/rock.glb".to_owned()));
        assert!(!names.iter().any(|n| n.contains("autosave")));
        let mut text = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("hill/content.json").unwrap(),
            &mut text,
        )
        .unwrap();
        assert!(text.contains("CC-BY-4.0") && text.contains("Kim"));

        let entry: serde_json::Value = serde_json::from_str(&done.entry).unwrap();
        assert_eq!(entry["id"], "hill");
        assert_eq!(entry["author"], "Kim");
        assert_eq!(entry["version"], "1.2.0");
        assert_eq!(entry["sha256"], done.sha256.as_str());
        assert_eq!(entry["bytes"], done.bytes);
        assert!(std::fs::read_to_string(&done.notes)
            .unwrap()
            .contains("community/worlds.json"));
        // Saved again with the same contents, the ZIP is the same.
        let again = export(&folder, &out).unwrap();
        assert_eq!(again.files, done.files);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn covers_are_the_middle_sixteen_by_nine_scaled_down() {
        // 2000 x 2000, left half red, right half blue.
        let (w, h) = (2000u32, 2000u32);
        let mut rgba = Vec::new();
        for _ in 0..h {
            for x in 0..w {
                rgba.extend(if x < w / 2 {
                    [255, 0, 0, 0]
                } else {
                    [0, 0, 255, 0]
                });
            }
        }
        let png = cover_png(w, h, &rgba).unwrap();
        let decoder = png::Decoder::new(std::io::Cursor::new(png));
        let mut reader = decoder.read_info().unwrap();
        let mut pixels = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut pixels).unwrap();
        assert_eq!((info.width, info.height), (1280, 720));
        assert_eq!(&pixels[..4], &[255, 0, 0, 255]);
        let last = pixels.len() - 4;
        assert_eq!(&pixels[last..], &[0, 0, 255, 255]);
        assert!(cover_png(4, 4, &[0; 64]).is_err());
    }

    #[test]
    fn an_unsaved_folder_is_refused() {
        let root = std::env::temp_dir().join(format!("sare-share-none-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        assert!(export(&root, &root.join("out")).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }
}
