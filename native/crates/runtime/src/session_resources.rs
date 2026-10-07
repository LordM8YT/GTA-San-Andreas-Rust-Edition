//! Prepare/download on a worker, upload incrementally, then switch atomically.
//! The offline world and local catalogs remain owned until disconnect restores them.
use super::*;
use sa_net::resources::{self, Share};
use std::{
    collections::VecDeque,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Mutex,
    },
};

#[derive(Clone)]
enum Endpoint {
    Host(std::net::SocketAddr),
    RelayHost(std::net::SocketAddr, bool),
    Join(std::net::SocketAddr),
    RelayJoin(std::net::SocketAddr, String),
}
impl Endpoint {
    fn is_host(&self) -> bool {
        matches!(self, Self::Host(_) | Self::RelayHost(..))
    }
    fn start(
        self,
        name: &str,
        fingerprint: String,
        share: Share,
    ) -> std::io::Result<(sa_net::Session, Option<sa_net::relay::Publication>)> {
        match self {
            Self::Host(address) => {
                sa_net::Session::host_resources(address, name, share).map(|s| (s, None))
            }
            Self::RelayHost(address, public) => {
                sa_net::Session::host_relay_resources(address, name, public, share)
                    .map(|(s, p)| (s, Some(p)))
            }
            Self::Join(address) => {
                sa_net::Session::join_resources(address, name, Some(fingerprint)).map(|s| (s, None))
            }
            Self::RelayJoin(address, code) => {
                sa_net::Session::join_relay_resources(address, &code, name, Some(fingerprint))
                    .map(|s| (s, None))
            }
        }
    }
}
pub(super) struct Job {
    cancel: Arc<AtomicBool>,
    progress: Arc<Mutex<String>>,
    result: mpsc::Receiver<Result<Prepared>>,
}
impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
struct Prepared {
    endpoint: Endpoint,
    share: Share,
    fingerprint: String,
    loader: sa_scene::WorldLoader,
    scene: Scene,
    cars: Vec<(String, Scene)>,
    peds: Vec<(String, sa_scene::ped::Ped)>,
    names: Vec<String>,
}
pub(super) fn cache_directory() -> PathBuf {
    if let Ok(directory) = std::env::var("LOCALAPPDATA") {
        PathBuf::from(directory).join("SAFreeroam/server-cache")
    } else if let Ok(directory) = std::env::var("XDG_CACHE_HOME") {
        PathBuf::from(directory).join("sa-freeroam/server-cache")
    } else if let Ok(directory) = std::env::var("HOME") {
        PathBuf::from(directory).join(".cache/sa-freeroam/server-cache")
    } else {
        std::env::temp_dir().join("sa-freeroam-server-cache")
    }
}
fn cancelled(cancel: &AtomicBool) -> Result<()> {
    anyhow::ensure!(!cancel.load(Ordering::Relaxed), "Join cancelled");
    Ok(())
}
fn prepare(
    game: &Path,
    mods: Option<&Path>,
    endpoint: Endpoint,
    allow_download: bool,
    cache: &Path,
    cancel: &AtomicBool,
    progress: &Mutex<String>,
) -> Result<Prepared> {
    cancelled(cancel)?;
    let share = if endpoint.is_host() {
        *progress.lock().unwrap() = "Preparing enabled server resources...".into();
        mods.map(sa_scene::share_resources)
            .transpose()?
            .unwrap_or_default()
    } else {
        Share::default()
    };
    let show_progress = |done: usize, total: usize| {
        *progress.lock().unwrap() = format!(
            "Preparing server mods: {:.1} / {:.1} MiB",
            done as f64 / 1048576.0,
            total as f64 / 1048576.0
        );
    };
    let cached = match &endpoint {
        Endpoint::Host(_) | Endpoint::RelayHost(..) => resources::cache(
            cache,
            &share.manifest,
            |file| {
                if cancel.load(Ordering::Relaxed) {
                    return Err(std::io::Error::other("Host cancelled"));
                }
                share
                    .blob(&file.sha256)
                    .map(|b| b.to_vec())
                    .ok_or_else(|| std::io::Error::other("Server resource disappeared"))
            },
            show_progress,
        )?,
        Endpoint::Join(address) => {
            resources::download(*address, None, cache, allow_download, cancel, show_progress)?
        }
        Endpoint::RelayJoin(address, code) => resources::download(
            *address,
            Some(code),
            cache,
            allow_download,
            cancel,
            show_progress,
        )?,
    };
    eprintln!(
        "Server resource cache: {} downloaded bytes, {} reused files, {}",
        cached.downloaded_bytes, cached.reused_files, cached.fingerprint
    );
    cancelled(cancel)?;
    *progress.lock().unwrap() = "Loading the server's world and models...".into();
    let mut loader = sa_scene::WorldLoader::open(game)?;
    loader.enable_mods(&cached.mods)?;
    let names = loader.mod_names().to_vec();
    let models = loader.model_names();
    let scene = loader.load(ORIGIN, ORIGIN, RADIUS)?;
    let mut cars = vec![(
        models.0,
        loader
            .take_custom_car()
            .map(Ok)
            .unwrap_or_else(|| sa_scene::load_car(game))?,
    )];
    cars.extend(loader.take_car_catalog());
    for (name, model) in [
        ("Taxi", "taxi"),
        ("Infernus", "infernus"),
        ("Admiral", "admiral"),
    ] {
        cancelled(cancel)?;
        cars.push((name.into(), sa_scene::load_car_model(game, model)?));
    }
    let mut peds = vec![(
        models.1,
        loader
            .take_custom_player(game)?
            .map(Ok)
            .unwrap_or_else(|| sa_scene::ped::Ped::load(game))?,
    )];
    peds.extend(loader.take_ped_catalog(game)?);
    for (name, model) in [
        ("Grove Street", "fam1"),
        ("Grove Street 2", "fam2"),
        ("Ballas", "ballas1"),
    ] {
        cancelled(cancel)?;
        peds.push((name.into(), sa_scene::ped::Ped::load_model(game, model)?));
    }
    cancelled(cancel)?;
    Ok(Prepared {
        endpoint,
        share,
        fingerprint: cached.fingerprint,
        loader,
        scene,
        cars,
        peds,
        names,
    })
}

enum Model {
    Car(String, f32),
    Ped(String, sa_scene::ped::Ped),
}
type CarMesh = (sa_scene::vehicle::Car, Vec<GpuBatch>);
type PedMesh = (sa_scene::ped::Ped, Vec<GpuBatch>);
pub(super) struct Installing {
    endpoint: Endpoint,
    share: Share,
    fingerprint: String,
    loader: Option<sa_scene::WorldLoader>,
    names: Vec<String>,
    world_upload: Option<upload::Upload>,
    world_ready: Option<(Vec<GpuBatch>, Scene)>,
    cars: VecDeque<(String, Scene)>,
    peds: VecDeque<(String, sa_scene::ped::Ped)>,
    current: Option<(Model, upload::Upload)>,
    car_meshes: Vec<(String, CarMesh)>,
    ped_meshes: Vec<(String, PedMesh)>,
}
impl Installing {
    fn new(prepared: Prepared) -> Self {
        Self {
            endpoint: prepared.endpoint,
            share: prepared.share,
            fingerprint: prepared.fingerprint,
            loader: Some(prepared.loader),
            names: prepared.names,
            world_upload: Some(upload::Upload::new(prepared.scene)),
            world_ready: None,
            cars: prepared.cars.into(),
            peds: prepared.peds.into(),
            current: None,
            car_meshes: Vec::new(),
            ped_meshes: Vec::new(),
        }
    }
    fn advance(&mut self, state: &State) -> Result<bool> {
        if let Some(mut upload) = self.world_upload.take() {
            if upload.advance(
                &state.device,
                &state.queue,
                &state.image_layout,
                &state.sampler,
            )? {
                self.world_ready = Some(upload.finish());
            } else {
                self.world_upload = Some(upload);
            }
            return Ok(false);
        }
        if self.current.is_none() {
            if let Some((name, scene)) = self.cars.pop_front() {
                let clearance = -scene
                    .batches
                    .iter()
                    .flat_map(|b| &b.vertices)
                    .map(|v| v.position[1])
                    .fold(f32::INFINITY, f32::min);
                anyhow::ensure!(clearance.is_finite(), "Server vehicle has no geometry");
                self.current = Some((Model::Car(name, clearance), upload::Upload::new(scene)));
            } else if let Some((name, ped)) = self.peds.pop_front() {
                let scene = ped.scene()?;
                self.current = Some((Model::Ped(name, ped), upload::Upload::new(scene)));
            } else {
                return Ok(true);
            }
        }
        if let Some((model, mut upload)) = self.current.take() {
            if upload.advance(
                &state.device,
                &state.queue,
                &state.image_layout,
                &state.sampler,
            )? {
                let (batches, _) = upload.finish();
                match model {
                    Model::Car(name, clearance) => self.car_meshes.push((
                        name,
                        (sa_scene::vehicle::Car::new(Vec3::ZERO, clearance), batches),
                    )),
                    Model::Ped(name, ped) => self.ped_meshes.push((name, (ped, batches))),
                }
            } else {
                self.current = Some((model, upload));
            }
        }
        Ok(false)
    }
    fn world(mut self) -> (World, Endpoint, Share, String) {
        let (batches, mut scene) = self.world_ready.take().unwrap();
        let car_names = self
            .car_meshes
            .iter()
            .map(|(name, _)| name.clone())
            .collect();
        let ped_names = self
            .ped_meshes
            .iter()
            .map(|(name, _)| name.clone())
            .collect();
        let mut cars: Vec<_> = self
            .car_meshes
            .into_iter()
            .map(|(_, car)| Some(car))
            .collect();
        let mut peds: Vec<_> = self
            .ped_meshes
            .into_iter()
            .map(|(_, ped)| Some(ped))
            .collect();
        let car = cars[0].take();
        let ped = peds[0].take();
        let clothes = ped.as_ref().unwrap().0.clothing_options();
        let world = World {
            batches,
            collision: scene.collision.take(),
            water: scene.water.take(),
            streamer: Some(Streamer::new(self.loader.take().unwrap())),
            uploading: None,
            retired: Vec::new(),
            car,
            ped,
            car_catalog: cars,
            ped_catalog: peds,
            active_car: 0,
            active_ped: 0,
            spawned_peds: Vec::new(),
            position: Vec3::new(-10.0, 40.0, -5.0),
            player: None,
            walking: true,
            driving: false,
            region: ORIGIN,
            interior: 0,
            destination: None,
            interior_destination: None,
            room_entry: None,
            yaw: 0.0,
            pitch: 0.0,
            mods: self.names,
            cars: car_names,
            peds: ped_names,
            clothes,
        };
        (world, self.endpoint, self.share, self.fingerprint)
    }
}
pub(super) struct World {
    batches: Vec<GpuBatch>,
    collision: Option<CollisionWorld>,
    water: Option<Arc<sa_scene::water::WaterMap>>,
    streamer: Option<Streamer>,
    uploading: Option<(streaming::Region, upload::Upload)>,
    retired: Vec<streaming::Retired>,
    car: Option<CarMesh>,
    ped: Option<PedMesh>,
    car_catalog: Vec<Option<CarMesh>>,
    ped_catalog: Vec<Option<PedMesh>>,
    active_car: usize,
    active_ped: usize,
    spawned_peds: Vec<SpawnedPed>,
    position: Vec3,
    player: Option<Player>,
    walking: bool,
    driving: bool,
    region: [f32; 2],
    interior: u8,
    destination: Option<[f32; 2]>,
    interior_destination: Option<usize>,
    room_entry: Option<Vec3>,
    yaw: f32,
    pitch: f32,
    mods: Vec<String>,
    cars: Vec<String>,
    peds: Vec<String>,
    clothes: Vec<(String, bool)>,
}
impl State {
    fn swap_session_world(&mut self, mut world: World) -> World {
        macro_rules! swap { ($($field:ident),*) => {$(std::mem::swap(&mut self.$field, &mut world.$field);)*}; }
        swap!(
            batches,
            collision,
            water,
            streamer,
            uploading,
            retired,
            car,
            ped,
            car_catalog,
            ped_catalog,
            active_car,
            active_ped,
            spawned_peds,
            position,
            player,
            walking,
            driving,
            region,
            interior,
            destination,
            interior_destination,
            room_entry,
            yaw,
            pitch
        );
        std::mem::swap(&mut self.menu.mods, &mut world.mods);
        std::mem::swap(&mut self.menu.cars, &mut world.cars);
        std::mem::swap(&mut self.menu.peds, &mut world.peds);
        std::mem::swap(&mut self.menu.clothes, &mut world.clothes);
        self.keys.clear();
        self.ped_clip = "idle_stance";
        self.ped_seconds = 0.0;
        self.ped_visible = false;
        world
    }
    pub(super) fn clear_session_resources(&mut self) {
        self.resource_job = None;
        self.resource_installing = None;
        if let Some(world) = self.offline_world.take() {
            let _ = self.swap_session_world(world);
            eprintln!("Restored offline world and local resources");
        }
        self.menu.network_ready = false;
    }
    pub(super) fn prepare_network(&mut self, host: bool) -> Result<()> {
        anyhow::ensure!(
            self.network_session.is_none()
                && self.resource_job.is_none()
                && self.resource_installing.is_none(),
            "Disconnect from the current session first."
        );
        self.network_car_spawned = false;
        let endpoint = if self.menu.relay_mode {
            let address = self
                .menu
                .relay_address
                .trim()
                .parse()
                .context("Use the relay's IP and port")?;
            if host {
                Endpoint::RelayHost(address, self.menu.public_session)
            } else {
                Endpoint::RelayJoin(address, self.menu.join_code.clone())
            }
        } else if host {
            Endpoint::Host(
                self.menu
                    .host_address
                    .trim()
                    .parse()
                    .context("Use a host IP and port")?,
            )
        } else {
            Endpoint::Join(
                self.menu
                    .join_address
                    .trim()
                    .parse()
                    .context("Use the host's IP and port")?,
            )
        };
        let game = self.resource_game_dir.clone();
        let mods = self.resource_local_mods.clone();
        let cache = self.resource_cache_dir.clone();
        let allow = self.menu.settings.auto_mod_downloads;
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(Mutex::new("Preparing multiplayer resources...".to_string()));
        let (sender, result) = mpsc::sync_channel(1);
        let (cancel_worker, progress_worker) = (cancel.clone(), progress.clone());
        std::thread::Builder::new()
            .name("session-resources".into())
            .spawn(move || {
                let prepared = prepare(
                    &game,
                    mods.as_deref(),
                    endpoint,
                    allow,
                    &cache,
                    &cancel_worker,
                    &progress_worker,
                );
                let _ = sender.send(prepared);
            })?;
        self.resource_job = Some(Job {
            cancel,
            progress,
            result,
        });
        self.menu.network_active = true;
        self.menu.network_ready = false;
        self.menu.message.clear();
        Ok(())
    }
    pub(super) fn update_session_resources(&mut self) {
        if let Some(job) = &self.resource_job {
            self.menu.network_status = job.progress.lock().unwrap().clone();
            match job.result.try_recv() {
                Ok(Ok(prepared)) => {
                    self.resource_installing = Some(Installing::new(prepared));
                    self.resource_job = None;
                }
                Ok(Err(error)) => {
                    self.resource_job = None;
                    self.menu.message = format!("Could not prepare server resources: {error:#}");
                    self.menu.network_status = "Offline".into();
                    self.menu.network_active = false;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.resource_job = None;
                    self.menu.network_active = false;
                    self.menu.message = "Resource worker stopped.".into();
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(mut installing) = self.resource_installing.take() {
            self.menu.network_status = "Preparing server models for display...".into();
            match installing.advance(self) {
                Ok(false) => self.resource_installing = Some(installing),
                Ok(true) => {
                    let (world, endpoint, share, fingerprint) = installing.world();
                    let joining = !endpoint.is_host();
                    self.offline_world = Some(self.swap_session_world(world));
                    // Use the known street spawn, rather than the streaming
                    // center whose highest surface can be a garage/roof.
                    if let Some(collision) = &self.collision {
                        let player = Player::spawn(collision, self.position);
                        self.position = player.eye();
                        self.player = Some(player);
                    }
                    self.place_car();
                    self.driving = false;
                    match endpoint.start(&self.menu.player_name, fingerprint, share) {
                        Ok((session, publication)) => {
                            eprintln!(
                                "Multiplayer {}: {}",
                                if joining { "joining" } else { "host listening" },
                                session.address
                            );
                            self.network_session = Some(session);
                            self.network_publication = publication;
                            self.network_revision = 0;
                        }
                        Err(error) => {
                            self.clear_session_resources();
                            self.menu.network_active = false;
                            self.menu.message = format!("Could not start session: {error}");
                        }
                    }
                }
                Err(error) => {
                    self.menu.network_active = false;
                    self.menu.network_status = "Offline".into();
                    self.menu.message = format!("Could not display server resources: {error:#}");
                }
            }
        }
    }
}
