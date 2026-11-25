//! The Asset Browser's catalog. Assets come from [`AssetProvider`]s: SARE's
//! built-in shapes, the open project's own folder and every installed
//! content pack. Categories are plain strings taken from the provider (for
//! folders: the folder under `assets/`), so new kinds of assets need no
//! change here. A provider for another source (a GTA adapter, an online
//! library) only has to implement the trait.
use sa_scene::builtin_assets;
use sa_scene::world_layout::{AssetRef, Layout};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq)]
pub struct AssetEntry {
    pub reference: AssetRef,
    pub label: String,
    pub category: String,
    /// Where it comes from, shown in the browser ("Built-in", a pack name).
    pub source: String,
    pub status: AssetStatus,
}
#[derive(Clone, Debug, PartialEq)]
pub enum AssetStatus {
    Ready,
    /// Used by the layout but not found on this computer.
    Missing(String),
    /// Found, but failed to load (set by the runtime when it tries).
    Invalid(String),
}

pub trait AssetProvider {
    fn scan(&self) -> Vec<AssetEntry>;
}

/// SARE's generated shapes.
pub struct Builtins;
impl AssetProvider for Builtins {
    fn scan(&self) -> Vec<AssetEntry> {
        builtin_assets::ALL
            .iter()
            .map(|b| AssetEntry {
                reference: AssetRef::Builtin(b.name.into()),
                label: b.label.into(),
                category: b.category.into(),
                source: "Built-in".into(),
                status: AssetStatus::Ready,
            })
            .collect()
    }
}

/// Every `.glb`/`.gltf` file in a folder tree: the project itself (`self:`
/// references) or another content pack (`pack:<id>:` references).
pub struct Folder {
    pub root: PathBuf,
    /// None for the open project, Some(id) for another pack.
    pub pack_id: Option<String>,
    pub source: String,
}
/// Deepest folder level scanned.
const MAX_DEPTH: usize = 6;
/// Most files one folder adds.
const MAX_FILES: usize = 2000;

impl AssetProvider for Folder {
    fn scan(&self) -> Vec<AssetEntry> {
        let mut files = Vec::new();
        collect(&self.root, &self.root, 0, &mut files);
        files.sort();
        files
            .into_iter()
            .take(MAX_FILES)
            .map(|relative| {
                let category = category_for(&relative);
                let label = Path::new(&relative)
                    .file_stem()
                    .map(|s| s.to_string_lossy().replace(['_', '-'], " "))
                    .unwrap_or_else(|| relative.clone());
                let reference = match &self.pack_id {
                    None => AssetRef::Own(relative),
                    Some(id) => AssetRef::Pack {
                        id: id.clone(),
                        path: relative,
                    },
                };
                AssetEntry {
                    reference,
                    label,
                    category,
                    source: self.source.clone(),
                    status: AssetStatus::Ready,
                }
            })
            .collect()
    }
}

fn collect(root: &Path, folder: &Path, depth: usize, out: &mut Vec<String>) {
    if depth > MAX_DEPTH || out.len() >= MAX_FILES {
        return;
    }
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect(root, &path, depth + 1, out);
        } else if name.to_ascii_lowercase().ends_with(".glb") {
            // Only .glb: a .gltf's side files make it fragile to place.
            if let Ok(relative) = path.strip_prefix(root) {
                let parts: Vec<_> = relative
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                out.push(parts.join("/"));
            }
        }
    }
}

/// `assets/<category>/x.glb` uses the folder name; known pack files get a
/// fitting category; anything else is "Models".
fn category_for(relative: &str) -> String {
    let parts: Vec<&str> = relative.split('/').collect();
    let title = |s: &str| {
        let mut c = s.chars();
        c.next()
            .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
            .unwrap_or_default()
            .replace(['_', '-'], " ")
    };
    match parts.as_slice() {
        ["assets", category, _, ..] => title(category),
        ["vehicles", ..] => "Vehicles".into(),
        [file] if file.starts_with("characters") => "Characters".into(),
        [file] if file.starts_with("world") => "Worlds".into(),
        [folder, _, ..] => title(folder),
        _ => "Models".into(),
    }
}

/// The world's saved models ("My Models": `models/*.model.json`).
pub struct SavedModels {
    pub root: PathBuf,
}
/// Category and source the Toolbox shows saved models under.
pub const MY_MODELS: &str = "My Models";

impl AssetProvider for SavedModels {
    fn scan(&self) -> Vec<AssetEntry> {
        let folder = self.root.join(sa_scene::saved_model::DIR);
        let Ok(entries) = std::fs::read_dir(&folder) else {
            return Vec::new();
        };
        let mut files: Vec<String> = entries
            .flatten()
            .filter_map(|e| e.file_name().to_str().map(str::to_owned))
            .filter(|name| sa_scene::saved_model::is_saved_model(name) && !name.starts_with('.'))
            .collect();
        files.sort();
        files
            .into_iter()
            .take(MAX_FILES)
            .map(|file| {
                let relative = format!("{}/{file}", sa_scene::saved_model::DIR);
                let read = sa_scene::saved_model::SavedModel::read(&folder.join(&file));
                let (label, status) = match read {
                    Ok(saved) => (saved.name().to_owned(), AssetStatus::Ready),
                    Err(error) => (file.clone(), AssetStatus::Invalid(format!("{error:#}"))),
                };
                AssetEntry {
                    reference: AssetRef::Own(relative),
                    label,
                    category: MY_MODELS.into(),
                    source: MY_MODELS.into(),
                    status,
                }
            })
            .collect()
    }
}

/// All assets the browser shows.
pub struct Catalog {
    pub entries: Vec<AssetEntry>,
}
impl Catalog {
    pub fn scan(providers: &[Box<dyn AssetProvider>]) -> Self {
        Self {
            entries: providers.iter().flat_map(|p| p.scan()).collect(),
        }
    }
    /// Providers for a project: built-ins, the project folder and every
    /// other content pack found in `pack_dirs`.
    pub fn providers_for(
        project: &Path,
        project_id: &str,
        pack_dirs: &[PathBuf],
    ) -> Vec<Box<dyn AssetProvider>> {
        let mut providers: Vec<Box<dyn AssetProvider>> = vec![
            Box::new(SavedModels {
                root: project.to_path_buf(),
            }),
            Box::new(Builtins),
            Box::new(Folder {
                root: project.to_path_buf(),
                pack_id: None,
                source: "This world".into(),
            }),
        ];
        let mut seen = BTreeSet::from([project_id.to_owned()]);
        for dir in pack_dirs {
            for (folder, manifest) in sa_scene::content_pack::list(dir) {
                if seen.insert(manifest.id.clone()) {
                    providers.push(Box::new(Folder {
                        root: folder,
                        pack_id: Some(manifest.id.clone()),
                        source: manifest.name.clone(),
                    }));
                }
            }
        }
        providers
    }
    /// Add an entry for every asset the layout uses that no provider has,
    /// so missing models show up in the browser instead of vanishing.
    pub fn add_missing(&mut self, layout: &Layout) {
        let known: BTreeSet<AssetRef> = self.entries.iter().map(|e| e.reference.clone()).collect();
        let missing: BTreeSet<AssetRef> = layout
            .nodes
            .iter()
            .filter_map(|n| n.asset.clone())
            .filter(|a| !known.contains(a))
            .collect();
        for reference in missing {
            self.entries.push(AssetEntry {
                label: reference.to_string(),
                status: AssetStatus::Missing(match &reference {
                    AssetRef::Builtin(name) => format!("this SARE has no built-in {name:?}"),
                    AssetRef::Own(path) => format!("{path} is not in this world's folder"),
                    AssetRef::Pack { id, path } => {
                        format!("{path} from content pack {id:?} is not installed")
                    }
                }),
                reference,
                category: "Missing".into(),
                source: "Missing".into(),
            });
        }
    }
    pub fn categories(&self) -> Vec<String> {
        let set: BTreeSet<&str> = self.entries.iter().map(|e| e.category.as_str()).collect();
        set.into_iter().map(str::to_owned).collect()
    }
    pub fn find(&self, reference: &AssetRef) -> Option<&AssetEntry> {
        self.entries.iter().find(|e| &e.reference == reference)
    }
    pub fn find_mut(&mut self, reference: &AssetRef) -> Option<&mut AssetEntry> {
        self.entries.iter_mut().find(|e| &e.reference == reference)
    }
    /// Entries matching a search text (label, category or source) and an
    /// optional category.
    pub fn search<'a>(
        &'a self,
        text: &str,
        category: Option<&'a str>,
    ) -> impl Iterator<Item = &'a AssetEntry> + 'a {
        let needle = text.trim().to_lowercase();
        self.entries.iter().filter(move |e| {
            category.is_none_or(|c| e.category == c)
                && (needle.is_empty()
                    || e.label.to_lowercase().contains(&needle)
                    || e.category.to_lowercase().contains(&needle)
                    || e.source.to_lowercase().contains(&needle))
        })
    }
}
