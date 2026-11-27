//! Build Mode projects. A project *is* a content pack folder: `content.json`
//! plus `layout.json` (and any `.glb` files the creator imported), so a saved
//! project plays with `sa-runtime --content <id>` and can be shared by
//! copying the folder. No absolute paths are stored anywhere.
use crate::history::{Document, WorldInfo};
use anyhow::{bail, ensure, Context, Result};
use sa_scene::content_pack::{Manifest, VehicleSpec, MANIFEST};
use sa_scene::pack_vehicle;
use sa_scene::world_layout::{self, write_atomic, AssetRef, Layout, Node, Transform};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Unsaved work written every few minutes, removed by a real save.
pub const AUTOSAVE: &str = ".autosave.json";
/// Where imported models are copied inside the project.
pub const ASSETS_DIR: &str = "assets";
/// Where imported vehicles are copied inside the project.
pub const VEHICLES_DIR: &str = "vehicles";

pub struct Project {
    pub folder: PathBuf,
    /// `content.json` as read, so fields Build Mode does not edit (vehicles,
    /// characters, unknown future fields) are written back untouched.
    manifest: Value,
    pub id: String,
    /// Layout file name inside the folder.
    layout_file: String,
}

/// Work found in an autosave that is newer than the saved project.
pub struct Recovery {
    pub document: Document,
    pub saved_at: String,
}

impl Project {
    /// Make a new project folder with a ground plate to stand on.
    pub fn create(folder: &Path, id: &str, name: &str) -> Result<(Self, Document)> {
        ensure!(
            !folder.join(MANIFEST).exists(),
            "{} already holds a world",
            folder.display()
        );
        let manifest = json!({
            "id": id,
            "name": name,
            "version": "0.1.0",
            "license": crate::share::DEFAULT_LICENSE,
            "description": "Made in SARE Studio.",
            "world": { "layout": world_layout::FILE, "spawn": [0.0, -6.0, 0.5] }
        });
        // Validate before touching the disk.
        let parsed: Manifest = serde_json::from_value(manifest.clone())
            .context("the new world's settings are invalid")?;
        check(&parsed)?;
        std::fs::create_dir_all(folder)
            .with_context(|| format!("creating {}", folder.display()))?;
        let mut layout = Layout::default();
        let ground = layout.allocate_id();
        layout.nodes.push(Node::object(
            ground,
            "Ground",
            AssetRef::Builtin("ground".into()),
            Transform::default(),
        ));
        let project = Self {
            folder: folder.to_path_buf(),
            manifest,
            id: id.to_owned(),
            layout_file: world_layout::FILE.into(),
        };
        let document = Document {
            layout,
            info: WorldInfo::new(name, "Made in SARE Studio.", [0.0, -6.0, 0.5]),
        };
        project.save(&document)?;
        Ok((project, document))
    }

    /// Open a pack folder for editing. A pack without a layout gets an empty
    /// one; its `world.glb` stays as the fixed base world.
    pub fn open(folder: &Path) -> Result<(Self, Document)> {
        let path = folder.join(MANIFEST);
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let manifest: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
            .with_context(|| format!("{} is not valid JSON", path.display()))?;
        let parsed = Manifest::read(folder)?;
        let layout_file = parsed
            .world
            .layout
            .clone()
            .unwrap_or_else(|| world_layout::FILE.into());
        let layout_path = sa_scene::content_pack::inside(folder, &layout_file)?;
        let layout = if layout_path.is_file() {
            Layout::read(&layout_path)?
        } else {
            ensure!(
                parsed.world.layout.is_none(),
                "{} names {layout_file}, but that file is missing",
                path.display()
            );
            Layout::default()
        };
        let document = Document {
            layout,
            info: WorldInfo {
                name: parsed.name.clone(),
                description: parsed.description.clone().unwrap_or_default(),
                spawn: parsed.world.spawn,
                author: parsed.author.clone().unwrap_or_default(),
                version: parsed.version.clone(),
                license: parsed.license.clone(),
                vehicles: parsed.vehicles.clone(),
            },
        };
        Ok((
            Self {
                folder: folder.to_path_buf(),
                manifest,
                id: parsed.id,
                layout_file,
            },
            document,
        ))
    }

    pub fn layout_path(&self) -> PathBuf {
        self.folder.join(&self.layout_file)
    }
    pub fn name(&self) -> &str {
        self.manifest["name"].as_str().unwrap_or(&self.id)
    }

    /// Write `content.json` and the layout. Both are checked first and
    /// written atomically; a failed save leaves the old files in place.
    pub fn save(&self, document: &Document) -> Result<()> {
        let mut manifest = self.manifest.clone();
        let object = manifest
            .as_object_mut()
            .context("content.json must be an object")?;
        object.insert("name".into(), Value::from(document.info.name.trim()));
        if document.info.description.trim().is_empty() {
            object.remove("description");
        } else {
            object.insert(
                "description".into(),
                Value::from(document.info.description.trim()),
            );
        }
        let author = document.info.author.trim();
        if author.is_empty() {
            object.remove("author");
        } else {
            object.insert("author".into(), Value::from(author));
        }
        object.insert("version".into(), Value::from(document.info.version.trim()));
        object.insert("license".into(), Value::from(document.info.license.trim()));
        if document.info.vehicles.is_empty() {
            object.remove("vehicles");
        } else {
            object.insert(
                "vehicles".into(),
                serde_json::to_value(&document.info.vehicles)?,
            );
        }
        let world = object
            .entry("world")
            .or_insert_with(|| json!({}))
            .as_object_mut()
            .context("world must be an object")?;
        world.insert("layout".into(), Value::from(self.layout_file.clone()));
        world.insert("spawn".into(), json!(document.info.spawn));
        let parsed: Manifest = serde_json::from_value(manifest.clone())
            .context("the world settings cannot be saved")?;
        check(&parsed)?;
        let layout_text = document.layout.to_json()?;
        write_atomic(&self.layout_path(), layout_text.as_bytes())?;
        write_atomic(
            &self.folder.join(MANIFEST),
            (serde_json::to_string_pretty(&manifest)? + "\n").as_bytes(),
        )?;
        let _ = std::fs::remove_file(self.folder.join(AUTOSAVE));
        Ok(())
    }

    /// Save a copy of the whole project (including imported models) as a
    /// new world in `folder` with a new id, and continue editing the copy.
    pub fn save_as(&self, document: &Document, folder: &Path, id: &str) -> Result<Self> {
        ensure!(
            !folder.join(MANIFEST).exists(),
            "{} already holds a world",
            folder.display()
        );
        copy_tree(&self.folder, folder)?;
        let mut manifest = self.manifest.clone();
        manifest["id"] = Value::from(id);
        let copy = Self {
            folder: folder.to_path_buf(),
            manifest,
            id: id.to_owned(),
            layout_file: self.layout_file.clone(),
        };
        copy.save(document)?;
        Ok(copy)
    }

    /// Write unsaved work next to the project without touching its files.
    pub fn autosave(&self, document: &Document) -> Result<()> {
        let info = &document.info;
        let doc = json!({
            "format": "sare-build-autosave",
            "saved_at": unix_seconds(),
            "info": {
                "name": info.name, "description": info.description, "spawn": info.spawn,
                "author": info.author, "version": info.version, "license": info.license,
                "vehicles": info.vehicles,
            },
            "layout": serde_json::to_value(&document.layout)?,
        });
        write_atomic(
            &self.folder.join(AUTOSAVE),
            serde_json::to_string(&doc)?.as_bytes(),
        )
    }

    /// Unsaved work from an earlier session that ended without saving.
    pub fn recovery(&self, saved: &Document) -> Result<Option<Recovery>> {
        let path = self.folder.join(AUTOSAVE);
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Ok(None);
        };
        let doc: Value = serde_json::from_str(&text).context("the autosave is damaged")?;
        ensure!(
            doc["format"] == "sare-build-autosave",
            "the autosave is not a SARE autosave"
        );
        let layout = Layout::from_json(&doc["layout"].to_string())?;
        let spawn: [f32; 3] = serde_json::from_value(doc["info"]["spawn"].clone())
            .context("the autosave has no spawn point")?;
        let document = Document {
            layout,
            info: WorldInfo {
                name: doc["info"]["name"].as_str().unwrap_or_default().to_owned(),
                description: doc["info"]["description"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
                spawn,
                // Autosaves from before these fields keep the saved ones.
                author: text_or(&doc["info"]["author"], &saved.info.author),
                version: text_or(&doc["info"]["version"], &saved.info.version),
                license: text_or(&doc["info"]["license"], &saved.info.license),
                vehicles: serde_json::from_value(doc["info"]["vehicles"].clone())
                    .unwrap_or_else(|_| saved.info.vehicles.clone()),
            },
        };
        if &document == saved {
            return Ok(None);
        }
        Ok(Some(Recovery {
            document,
            saved_at: doc["saved_at"].to_string(),
        }))
    }
    /// Drop the autosave (the creator chose to keep the saved version).
    pub fn discard_autosave(&self) {
        let _ = std::fs::remove_file(self.folder.join(AUTOSAVE));
    }

    /// Copy a `.glb`/`.gltf` model into the project's `assets/<category>/`
    /// folder, so the project stays self-contained, and return its reference.
    pub fn import_model(&self, source: &Path, category: &str) -> Result<AssetRef> {
        let name = source
            .file_name()
            .and_then(|n| n.to_str())
            .context("that path has no file name")?;
        ensure!(
            sa_scene::gltf_model::is_gltf_path(name),
            "only .glb and .gltf models can be imported"
        );
        // Check it loads before copying anything.
        sa_scene::gltf_model::load(source, "import-check")
            .with_context(|| format!("{name} cannot be used"))?;
        let category = sanitize(category);
        let category = if category.is_empty() {
            "custom".into()
        } else {
            category
        };
        if name.to_ascii_lowercase().ends_with(".gltf") {
            bail!("import .glb files (a .gltf needs its .bin and textures copied too)");
        }
        let bytes =
            std::fs::read(source).with_context(|| format!("reading {}", source.display()))?;
        let file = sanitize_file(name);
        let (stem, extension) = file.rsplit_once('.').unwrap_or((&file, "glb"));
        // The same model imported twice is the same asset; a different model
        // with the same name gets a numbered name.
        let mut number = 1;
        let (relative, target) = loop {
            let file = match number {
                1 => file.clone(),
                n => format!("{stem}-{n}.{extension}"),
            };
            let relative = format!("{ASSETS_DIR}/{category}/{file}");
            let target = self.folder.join(&relative);
            match std::fs::read(&target) {
                Ok(existing) if existing == bytes => return Ok(AssetRef::Own(relative)),
                Ok(_) => number += 1,
                Err(_) => break (relative, target),
            }
        };
        std::fs::create_dir_all(target.parent().unwrap())?;
        std::fs::write(&target, &bytes)
            .with_context(|| format!("copying {} into the world", source.display()))?;
        Ok(AssetRef::Own(relative))
    }

    /// Copy a vehicle `.glb` into the world's `vehicles/` folder and make its
    /// entry: a model name not in `taken`, a name from the file, default
    /// handling. The model is loaded first, so a broken file copies nothing.
    pub fn import_vehicle(
        &self,
        source: &Path,
        taken: &[VehicleSpec],
    ) -> Result<(VehicleSpec, pack_vehicle::Report)> {
        let name = source
            .file_name()
            .and_then(|n| n.to_str())
            .context("that path has no file name")?;
        ensure!(
            name.to_ascii_lowercase().ends_with(".glb"),
            "import a .glb vehicle (one file with its textures inside)"
        );
        let bytes =
            std::fs::read(source).with_context(|| format!("reading {}", source.display()))?;
        ensure!(
            bytes.len() <= 16 * 1024 * 1024,
            "{name} is over 16 MiB; servers cannot send it to players"
        );
        let scene = pack_vehicle::parse(
            &bytes,
            source.parent().unwrap_or(Path::new(".")),
            "import-check",
            [200, 200, 200],
            Default::default(),
        )
        .with_context(|| format!("{name} cannot be used as a vehicle"))?;
        let report = pack_vehicle::Report::of(&scene);
        let stem = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
        let base: String = match sanitize(stem) {
            id if id.is_empty() => "vehicle".into(),
            id => id.chars().take(28).collect(),
        };
        let model = (1..)
            .map(|n| match n {
                1 => base.clone(),
                n => format!("{base}-{n}"),
            })
            // A file left by a removed vehicle stays, so undo still finds it.
            .find(|m| {
                taken.iter().all(|t| &t.model != m)
                    && std::fs::read(self.folder.join(format!("{VEHICLES_DIR}/{m}.glb")))
                        .ok()
                        .is_none_or(|old| old == bytes)
            })
            .unwrap();
        let relative = format!("{VEHICLES_DIR}/{model}.glb");
        let target = self.folder.join(&relative);
        std::fs::create_dir_all(target.parent().unwrap())?;
        std::fs::write(&target, &bytes)
            .with_context(|| format!("copying {} into the world", source.display()))?;
        let title: String = stem
            .replace(['_', '-'], " ")
            .split_whitespace()
            .map(|word| {
                let mut chars = word.chars();
                chars
                    .next()
                    .map(|c| c.to_uppercase().collect::<String>() + chars.as_str())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join(" ");
        let spec = VehicleSpec {
            model: model.clone(),
            name: if title.trim().is_empty() {
                model
            } else {
                title.chars().take(48).collect()
            },
            scene: relative,
            color: Some([200, 200, 200]),
            handling: None,
            seat: None,
        };
        spec.validate()?;
        Ok((spec, report))
    }
}

fn text_or(value: &Value, fallback: &str) -> String {
    value.as_str().unwrap_or(fallback).to_owned()
}

fn check(manifest: &Manifest) -> Result<()> {
    manifest.validate()
}

/// Lowercase letters, digits, `-` and `_` only.
pub fn sanitize(text: &str) -> String {
    text.trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .take(64)
        .collect()
}
fn sanitize_file(name: &str) -> String {
    let (stem, extension) = name.rsplit_once('.').unwrap_or((name, "glb"));
    format!("{}.{}", sanitize(stem), extension.to_ascii_lowercase())
}

/// A free pack id based on `name` inside `packs_dir`.
pub fn new_id(name: &str, packs_dir: &Path) -> String {
    let base = match sanitize(name) {
        id if id.is_empty() => "my-world".to_owned(),
        id => id,
    };
    if !packs_dir.join(&base).exists() {
        return base;
    }
    (2..)
        .map(|i| format!("{base}-{i}"))
        .find(|id| !packs_dir.join(id).exists())
        .unwrap()
}

pub(crate) fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let name = entry.file_name();
        if name == AUTOSAVE {
            continue;
        }
        let target = to.join(&name);
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
