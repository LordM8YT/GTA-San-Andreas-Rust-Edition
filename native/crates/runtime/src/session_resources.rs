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
    /// sa-runtime arguments that join this endpoint again after a restart.
    fn join_args(&self) -> Option<Vec<String>> {
        match self {
            Self::Join(address) => Some(vec!["--join".into(), address.to_string()]),
            Self::RelayJoin(address, code) => Some(vec![
                "--relay-address".into(),
                address.to_string(),
                "--join-code".into(),
                code.clone(),
            ]),
            Self::Host(_) | Self::RelayHost(..) => None,
        }
    }
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
    cache_lease: Arc<std::fs::File>,
    endpoint: Endpoint,
    share: Share,
    fingerprint: String,
    /// The San Andreas world for the session. `None` in a standalone world
    /// (content pack or testbed): the player stays in the world already open.
    loader: Option<sa_scene::WorldLoader>,
    scene: Option<Scene>,
    cars: Vec<(String, Scene)>,
    peds: Vec<(String, sa_scene::ped::Ped)>,
    names: Vec<String>,
    /// The server runs on this content pack, which is not the open world.
    switch_to: Option<PackSwitch>,
    /// In a standalone world: the server's addon vehicles, in the order of
    /// `cars`.
    addons: Vec<sa_scene::content_pack::VehicleSpec>,
}
#[derive(Debug, PartialEq)]
struct PackSwitch {
    id: String,
    /// The world to restart in: the server's copy, downloaded, or else the
    /// player's own copy. `None`: neither is available.
    folder: Option<PathBuf>,
    /// Why the server's copy could not be fetched.
    problem: Option<String>,
}
/// The player's own copy of content pack `id`, if SARE can find one.
fn local_pack(id: &str) -> Option<PathBuf> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match sa_scene::content::ContentSource::from_args(&pack_lookup_args(&args, id), None) {
        Ok(sa_scene::content::ContentSource::Pack(folder)) => Some(folder),
        _ => None,
    }
}
/// Get the world `id` the server at `endpoint` plays on.
fn fetch_world(
    id: String,
    endpoint: &Endpoint,
    cache: &Path,
    allow_download: bool,
    cancel: &AtomicBool,
    progress: &Mutex<String>,
) -> PackSwitch {
    let local = local_pack(&id);
    let (address, code) = match endpoint {
        Endpoint::Join(address) => (*address, None),
        Endpoint::RelayJoin(address, code) => (*address, Some(code.as_str())),
        Endpoint::Host(_) | Endpoint::RelayHost(..) => {
            return PackSwitch {
                id,
                folder: local,
                problem: None,
            }
        }
    };
    *progress.lock().unwrap() = format!("Getting the server's world \"{id}\"...");
    match resources::download_world(
        address,
        code,
        &id,
        &resources::worlds_dir(cache),
        local.as_deref(),
        allow_download,
        cancel,
        |done, total| {
            *progress.lock().unwrap() = format!(
                "Downloading the server's world: {:.1} / {:.1} MiB",
                done as f64 / 1048576.0,
                total as f64 / 1048576.0
            );
        },
    ) {
        Ok(folder) => PackSwitch {
            id,
            folder: Some(folder),
            problem: None,
        },
        Err(error) => PackSwitch {
            id,
            folder: local,
            problem: Some(error.to_string()),
        },
    }
}
pub(super) fn cache_directory() -> PathBuf {
    sa_client::cache_dir()
}
fn cancelled(cancel: &AtomicBool) -> Result<()> {
    anyhow::ensure!(!cancel.load(Ordering::Relaxed), "Join cancelled");
    Ok(())
}
/// The world this SARE has open, which decides what a session loads.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum OpenWorld {
    /// The San Andreas installation in this folder.
    Original(PathBuf),
    /// A content pack (with its id) or the testbed (`None`): no game files.
    Standalone(Option<String>),
}
/// What a joining player must do when the server's world is not the open
/// one: `Some(pack)` restarts SARE in that content pack.
fn needs_pack(open: &OpenWorld, server_pack: Option<&str>) -> Option<String> {
    let server_pack = server_pack?;
    match open {
        OpenWorld::Standalone(Some(mine)) if mine == server_pack => None,
        _ => Some(server_pack.to_owned()),
    }
}
fn prepare(
    open: &OpenWorld,
    mods: Option<&Path>,
    endpoint: Endpoint,
    allow_download: bool,
    cache: &Path,
    cancel: &AtomicBool,
    progress: &Mutex<String>,
) -> Result<Prepared> {
    cancelled(cancel)?;
    let mut share = if endpoint.is_host() {
        *progress.lock().unwrap() = "Preparing enabled server resources...".into();
        mods.map(sa_scene::share_resources)
            .transpose()?
            .unwrap_or_default()
    } else {
        Share::default()
    };
    if let OpenWorld::Standalone(Some(pack)) = open {
        // A player hosting in a content pack tells joiners which world to
        // use, and hands it to those who do not have it.
        share.manifest.content_pack = Some(pack.clone());
        if endpoint.is_host() {
            if let Some(folder) = local_pack(pack) {
                if let Err(error) = share.add_world(pack, &folder) {
                    eprintln!("Cannot share world {pack} with players: {error}");
                }
            }
        }
    }
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
    let names = cached
        .manifest
        .resources
        .iter()
        .map(|r| {
            format!(
                "{} - verified for session - {:.2} MiB ({} files)",
                r.name,
                r.files.iter().map(|f| f.bytes).sum::<usize>() as f64 / 1048576.,
                r.files.len()
            )
        })
        .collect();
    let switch_to = if endpoint.is_host() {
        None
    } else {
        needs_pack(open, cached.manifest.content_pack.as_deref())
            .map(|id| fetch_world(id, &endpoint, cache, allow_download, cancel, progress))
    };
    cancelled(cancel)?;
    let game = match open {
        OpenWorld::Original(game) if switch_to.is_none() => game,
        // Content packs and the testbed have no San Andreas files to load;
        // the session runs in the world that is already open. A server on
        // another content pack restarts SARE in that pack instead.
        _ => {
            // The server's addon vehicles join the open world's car list.
            let (addons, cars) = if switch_to.is_some() {
                (Vec::new(), Vec::new())
            } else {
                *progress.lock().unwrap() = "Loading the server's vehicles...".into();
                let loaded = sa_scene::addon_vehicles(&cached.mods)
                    .context("the server's addon vehicles")?;
                loaded
                    .into_iter()
                    .map(|(spec, scene)| {
                        let name = spec.name.trim().to_owned();
                        (spec, (name, scene))
                    })
                    .unzip()
            };
            return Ok(Prepared {
                cache_lease: cached.lease,
                endpoint,
                share,
                fingerprint: cached.fingerprint,
                loader: None,
                scene: None,
                cars,
                peds: Vec::new(),
                names,
                switch_to,
                addons,
            });
        }
    };
    *progress.lock().unwrap() = "Loading the server's world and models...".into();
    let mut loader = sa_scene::WorldLoader::open(game)?;
    loader.enable_mods(&cached.mods)?;
    let models = loader.model_names();
    let scene = loader.load(ORIGIN, ORIGIN, RADIUS)?;
    let mut cars = vec![(
        models.0,
        loader
            .take_custom_car()
            .map(Ok)
            .unwrap_or_else(|| loader.load_original_car(game, "taxi"))?,
    )];
    cars.extend(loader.take_car_catalog());
    for (name, model) in loader.additional_original_car_models() {
        cancelled(cancel)?;
        cars.push((name.into(), loader.load_original_car(game, model)?));
    }
    let mut peds = vec![(
        models.1,
        loader
            .take_custom_player(game)?
            .map(Ok)
            .unwrap_or_else(|| sa_scene::ped::Ped::load(game))?,
    )];
    peds.extend(loader.take_ped_catalog(game)?);
    for (name, model) in loader.additional_original_ped_models() {
        cancelled(cancel)?;
        peds.push((name.into(), sa_scene::ped::Ped::load_model(game, model)?));
    }
    cancelled(cancel)?;
    Ok(Prepared {
        cache_lease: cached.lease,
        endpoint,
        share,
        fingerprint: cached.fingerprint,
        loader: Some(loader),
        scene: Some(scene),
        switch_to: None,
        cars,
        peds,
        names,
        addons: Vec::new(),
    })
}

enum Model {
    Car(String, f32, [f32; 3], sa_scene::vehicle::Handling),
    Ped(String, sa_scene::ped::Ped),
}
type CarMesh = (sa_scene::vehicle::Car, Vec<GpuBatch>);
type PedMesh = (sa_scene::ped::Ped, Vec<GpuBatch>);
pub(super) struct Installing {
    cache_lease: Arc<std::fs::File>,
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
    addons: Vec<sa_scene::content_pack::VehicleSpec>,
}
impl Installing {
    fn new(prepared: Prepared) -> Self {
        Self {
            cache_lease: prepared.cache_lease,
            endpoint: prepared.endpoint,
            share: prepared.share,
            fingerprint: prepared.fingerprint,
            loader: prepared.loader,
            names: prepared.names,
            world_upload: prepared.scene.map(upload::Upload::new),
            world_ready: None,
            cars: prepared.cars.into(),
            peds: prepared.peds.into(),
            current: None,
            car_meshes: Vec::new(),
            ped_meshes: Vec::new(),
            addons: prepared.addons,
        }
    }
    fn advance(&mut self, state: &State) -> Result<bool> {
        if let Some(mut upload) = self.world_upload.take() {
            if upload.advance(
                &state.device,
                &state.queue,
                &state.image_layout,
                &state.sampler,
                &state.mipmaps,
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
                let size = sa_scene::vehicle::measure(
                    scene
                        .batches
                        .iter()
                        .flat_map(|b| &b.vertices)
                        .map(|v| v.position),
                );
                self.current = Some((
                    Model::Car(name, clearance, size, scene.vehicle_handling),
                    upload::Upload::new(scene),
                ));
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
                &state.mipmaps,
            )? {
                let (batches, _) = upload.finish();
                match model {
                    Model::Car(name, clearance, size, handling) => self.car_meshes.push((
                        name,
                        (
                            sa_scene::vehicle::Car::new(Vec3::ZERO, clearance)
                                .with_handling(handling)
                                .with_size(size),
                            batches,
                        ),
                    )),
                    Model::Ped(name, ped) => self.ped_meshes.push((name, (ped, batches))),
                }
            } else {
                self.current = Some((model, upload));
            }
        }
        Ok(false)
    }
    /// Standalone sessions keep the open world: only the connection starts.
    fn has_world(&self) -> bool {
        self.loader.is_some()
    }
    /// The uploaded addon vehicles of a standalone session, with their specs.
    fn take_addon_cars(&mut self) -> Vec<(sa_scene::content_pack::VehicleSpec, String, CarMesh)> {
        std::mem::take(&mut self.addons)
            .into_iter()
            .zip(std::mem::take(&mut self.car_meshes))
            .map(|(spec, (name, mesh))| (spec, name, mesh))
            .collect()
    }
    fn connection(self) -> (Endpoint, Share, String) {
        (self.endpoint, self.share, self.fingerprint)
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
            cache_lease: Some(self.cache_lease),
            batches,
            collision: scene.collision.take(),
            water: scene.water.take(),
            map_spawns: self
                .loader
                .as_ref()
                .map(|l| l.map_spawns().to_vec())
                .unwrap_or_default(),
            pickups: crate::pickups::Pickups::new(
                self.loader.as_ref().map_or(&[][..], |l| l.map_pickups()),
            ),
            streamer: Some(Streamer::new(Box::new(self.loader.take().unwrap()))),
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
    cache_lease: Option<Arc<std::fs::File>>,
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
    map_spawns: Vec<(String, [f32; 3])>,
    pickups: crate::pickups::Pickups,
}
impl State {
    fn swap_session_world(&mut self, mut world: World) -> World {
        macro_rules! swap { ($($field:ident),*) => {$(std::mem::swap(&mut self.$field, &mut world.$field);)*}; }
        swap!(
            cache_lease,
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
            pitch,
            map_spawns,
            pickups
        );
        std::mem::swap(&mut self.menu.mods, &mut world.mods);
        std::mem::swap(&mut self.menu.cars, &mut world.cars);
        std::mem::swap(&mut self.menu.peds, &mut world.peds);
        std::mem::swap(&mut self.menu.clothes, &mut world.clothes);
        // Every client lists the original cars and peds after the session's own, so the
        // indices peers send name the same car.
        crate::append_originals(
            &mut self.car_catalog,
            &mut self.menu.cars,
            &self.original_car_models,
        );
        crate::append_originals(
            &mut self.ped_catalog,
            &mut self.menu.peds,
            &self.original_ped_models,
        );
        crate::metrics::session_memory("world swap");
        self.keys.clear();
        self.car_render_pose = None;
        self.ped_animator.reset();
        self.entering_car = None;
        self.ped_visible = false;
        world
    }
    /// Add a server's addon vehicles after the open world's own cars, the
    /// same way on every player, so the car indices peers send agree.
    fn add_addon_cars(
        &mut self,
        cars: Vec<(sa_scene::content_pack::VehicleSpec, String, CarMesh)>,
    ) {
        if cars.is_empty() {
            return;
        }
        let first = self.car_catalog.len();
        let mut names = Vec::new();
        for (spec, mut name, mesh) in cars {
            // A menu name the world already uses would share its seat.
            if self.menu.cars.contains(&name) {
                name = format!("{name} ({})", spec.model);
            }
            self.car_catalog.push(Some(mesh));
            self.menu.cars.push(name.clone());
            // Scripts spawn them by model name: CreateVehicle("van", ...).
            self.original_car_models
                .insert(name.clone(), spec.model.clone());
            self.pack
                .seats
                .insert(name.clone(), spec.seat_model_space());
            names.push(name);
        }
        crate::runtime_log::line(format!("Server vehicles added: {}", names.join(", ")));
        self.addon_cars = Some((first, names));
    }
    /// Take the server's addon vehicles out again, leaving any one driven.
    fn remove_addon_cars(&mut self) {
        let Some((first, names)) = self.addon_cars.take() else {
            return;
        };
        if self.active_car >= first {
            if let Some(own) = self.car_catalog.get_mut(0).and_then(Option::take) {
                let (position, yaw) = self
                    .car
                    .as_ref()
                    .map(|(car, _)| (car.position - Vec3::Y * car.clearance, car.yaw))
                    .unwrap_or((self.position, 0.0));
                self.car_catalog[self.active_car] = self.car.replace(own);
                self.active_car = 0;
                if let Some((car, _)) = &mut self.car {
                    car.stop();
                    car.position = position + Vec3::Y * car.clearance;
                    car.yaw = yaw;
                }
                self.car_render_pose = None;
            }
        }
        self.traffic.cars.retain(|car| car.catalog < first);
        self.car_catalog.truncate(first);
        self.menu.cars.truncate(first);
        for name in names {
            self.original_car_models.remove(&name);
            self.pack.seats.remove(&name);
        }
    }
    pub(super) fn clear_session_resources(&mut self) {
        self.resource_job = None;
        self.resource_installing = None;
        self.remove_addon_cars();
        if let Some(world) = self.offline_world.take() {
            let _ = self.swap_session_world(world);
            crate::metrics::session_memory("offline restored");
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
        self.checkpoint_progress();
        crate::metrics::session_memory("before preparation");
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
                resolve_address(&self.menu.join_address)
                    .context("Use the host's IP or address and port")?,
            )
        };
        // Content packs and the testbed have no San Andreas files.
        let open = if self.standalone {
            OpenWorld::Standalone(self.pack_id.clone())
        } else {
            OpenWorld::Original(self.resource_game_dir.clone())
        };
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
                    &open,
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
    /// The server plays on a content pack that is not open here: start SARE
    /// again in that pack and join from there, or say how to get the pack.
    fn switch_to_pack(&mut self, switch: PackSwitch, endpoint: &Endpoint) {
        self.menu.network_status = "Offline".into();
        self.menu.network_active = false;
        let args: Vec<String> = std::env::args().skip(1).collect();
        let pack = switch.id;
        let Some(folder) = switch.folder else {
            self.menu.message = format!(
                "This server plays on the world \"{pack}\", and it could not be downloaded ({}). Copy its folder into SARE's content folder and join again.",
                switch.problem.as_deref().unwrap_or("the server does not share it")
            );
            crate::runtime_log::line(&self.menu.message);
            return;
        };
        if let Some(problem) = &switch.problem {
            crate::runtime_log::line(format!(
                "Using your own copy of \"{pack}\": the server's could not be downloaded ({problem})."
            ));
        }
        let Some(join) = endpoint.join_args() else {
            return;
        };
        self.menu.message =
            format!("This server plays on \"{pack}\". Restarting SARE in that world...");
        crate::runtime_log::line(&self.menu.message);
        self.pack_restart = Some(restart_args(&args, &folder.to_string_lossy(), &join));
        self.quit_requested = true;
    }
    pub(super) fn update_session_resources(&mut self) {
        if let Some(job) = &self.resource_job {
            self.menu.network_status = job.progress.lock().unwrap().clone();
            match job.result.try_recv() {
                Ok(Ok(mut prepared)) if prepared.switch_to.is_some() => {
                    self.resource_job = None;
                    if let Some(switch) = prepared.switch_to.take() {
                        self.switch_to_pack(switch, &prepared.endpoint);
                    }
                }
                Ok(Ok(prepared)) => {
                    self.resource_installing = Some(Installing::new(prepared));
                    self.resource_job = None;
                }
                Ok(Err(error)) => {
                    self.resource_job = None;
                    self.menu.message = sa_client::diagnostics::explain(&format!(
                        "Could not prepare server resources: {error:#}"
                    ));
                    crate::runtime_log::line(&self.menu.message);
                    self.menu.network_status = "Offline".into();
                    self.menu.network_active = false;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.resource_job = None;
                    self.menu.network_active = false;
                    self.menu.message = "Resource worker stopped.".into();
                    crate::runtime_log::line(&self.menu.message);
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(mut installing) = self.resource_installing.take() {
            self.menu.network_status = "Preparing server models for display...".into();
            match installing.advance(self) {
                Ok(false) => self.resource_installing = Some(installing),
                Ok(true) => {
                    let (endpoint, share, fingerprint) = if installing.has_world() {
                        let (world, endpoint, share, fingerprint) = installing.world();
                        self.offline_world = Some(self.swap_session_world(world));
                        // Use the known street spawn, rather than the streaming
                        // center whose highest surface can be a garage/roof.
                        if let Some(collision) = &self.collision {
                            let player = Player::spawn(collision, self.position);
                            self.position = player.eye();
                            self.player = Some(player);
                        }
                        self.car_at_start();
                        (endpoint, share, fingerprint)
                    } else {
                        let addons = installing.take_addon_cars();
                        self.add_addon_cars(addons);
                        installing.connection()
                    };
                    let joining = !endpoint.is_host();
                    match endpoint.start(&self.menu.player_name, fingerprint, share) {
                        Ok((session, publication)) => {
                            crate::runtime_log::line(format!(
                                "Multiplayer {}: {}",
                                if joining { "joining" } else { "host listening" },
                                session.address
                            ));
                            self.network_session = Some(session);
                            self.network_host = !joining;
                            self.chat.reset_server_state();
                            self.network_publication = publication;
                            self.network_revision = 0;
                        }
                        Err(error) => {
                            self.clear_session_resources();
                            self.menu.network_active = false;
                            self.menu.message = sa_client::diagnostics::explain(&format!(
                                "Could not start session: {error}"
                            ));
                            crate::runtime_log::line(&self.menu.message);
                        }
                    }
                }
                Err(error) => {
                    self.menu.network_active = false;
                    self.menu.network_status = "Offline".into();
                    self.menu.message = format!("Could not display server resources: {error:#}");
                    crate::runtime_log::line(&self.menu.message);
                }
            }
        }
    }
}

/// Arguments that find `pack` the way this SARE finds content packs.
fn pack_lookup_args(args: &[String], pack: &str) -> Vec<String> {
    let mut lookup = vec!["--content".to_owned(), pack.to_owned()];
    if let Some(pair) = args.windows(2).find(|w| w[0] == "--content-dir") {
        lookup.extend(pair.iter().cloned());
    }
    lookup
}
/// This SARE's arguments with the world swapped for `pack` and the session
/// to join: the game folder, world, Build Mode and old join are dropped.
pub(crate) fn restart_args(args: &[String], pack: &str, join: &[String]) -> Vec<String> {
    let mut next = Vec::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--game-dir" | "--content" | "--build" | "--join" | "--join-code" | "--host"
            | "--relay-address" => {
                rest.next();
            }
            "--relay-host" | "--public-session" => {}
            _ => next.push(arg.clone()),
        }
    }
    next.extend(["--content".to_owned(), pack.to_owned()]);
    next.extend(join.iter().cloned());
    next
}
/// A host as `ip:port` or `name:port`, so tunnel addresses such as
/// playit.gg's `something.joinmc.link:12345` work as well as plain IPs.
/// `0.0.0.0` is where a server listens, not an address to connect to
/// (Windows refuses it), so it means this computer.
pub(crate) fn resolve_address(text: &str) -> Option<std::net::SocketAddr> {
    use std::net::ToSocketAddrs;
    let text = text.trim();
    let mut address: std::net::SocketAddr = text.parse().ok().or_else(|| {
        let mut found: Vec<_> = text.to_socket_addrs().ok()?.collect();
        found.sort_by_key(|a| !a.is_ipv4());
        found.into_iter().next()
    })?;
    if address.ip().is_unspecified() {
        address.set_ip(match address {
            std::net::SocketAddr::V4(_) => std::net::Ipv4Addr::LOCALHOST.into(),
            std::net::SocketAddr::V6(_) => std::net::Ipv6Addr::LOCALHOST.into(),
        });
    }
    Some(address)
}

#[cfg(test)]
mod address_tests {
    #[test]
    fn plain_and_named_hosts_resolve() {
        assert_eq!(
            super::resolve_address(" 127.0.0.1:7777 ").unwrap().port(),
            7777
        );
        assert_eq!(
            super::resolve_address("localhost:7777").unwrap().port(),
            7777
        );
        assert!(super::resolve_address("no port").is_none());
        assert_eq!(
            super::resolve_address("0.0.0.0:7777").unwrap(),
            "127.0.0.1:7777".parse().unwrap()
        );
    }
}

#[cfg(test)]
mod standalone_tests {
    use super::*;
    #[test]
    fn content_packs_host_without_game_files() {
        let cache =
            std::env::temp_dir().join(format!("sare-standalone-session-{}", std::process::id()));
        let prepared = prepare(
            &OpenWorld::Standalone(Some("rp-town".into())),
            None,
            Endpoint::Host("127.0.0.1:0".parse().unwrap()),
            false,
            &cache,
            &AtomicBool::new(false),
            &Mutex::new(String::new()),
        )
        .unwrap();
        assert!(prepared.loader.is_none() && prepared.scene.is_none());
        assert!(prepared.cars.is_empty() && prepared.peds.is_empty());
        assert_eq!(prepared.switch_to, None);
        let installing = Installing::new(prepared);
        assert!(!installing.has_world());
        let (endpoint, share, _) = installing.connection();
        assert!(endpoint.is_host());
        assert_eq!(share.manifest.content_pack.as_deref(), Some("rp-town"));
        let _ = std::fs::remove_dir_all(cache);
    }
    #[test]
    fn a_world_that_cannot_be_fetched_says_why() {
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = closed.local_addr().unwrap();
        drop(closed);
        let cache = std::env::temp_dir().join(format!("sare-world-fetch-{}", std::process::id()));
        let switch = fetch_world(
            "no-such-world-here".into(),
            &Endpoint::Join(address),
            &cache.join("server-cache"),
            true,
            &AtomicBool::new(false),
            &Mutex::new(String::new()),
        );
        assert_eq!(switch.folder, None);
        assert!(switch.problem.is_some());
        let _ = std::fs::remove_dir_all(cache);
    }
    #[test]
    fn joining_follows_the_server_content_pack() {
        let original = OpenWorld::Original(PathBuf::from("gta"));
        let town = OpenWorld::Standalone(Some("sare-rp-town".into()));
        assert_eq!(needs_pack(&original, None), None);
        assert_eq!(
            needs_pack(&original, Some("sare-rp-town")).as_deref(),
            Some("sare-rp-town")
        );
        assert_eq!(needs_pack(&town, Some("sare-rp-town")), None);
        assert_eq!(needs_pack(&town, Some("other")).as_deref(), Some("other"));
        assert_eq!(
            needs_pack(&OpenWorld::Standalone(None), Some("x")).as_deref(),
            Some("x")
        );
        // A server without a pack keeps whatever world is open.
        assert_eq!(needs_pack(&town, None), None);
    }
    #[test]
    fn restart_swaps_the_world_and_keeps_the_rest() {
        let args: Vec<String> = [
            "--game-dir",
            "E:/GTA",
            "--mods-dir",
            "mods",
            "--name",
            "Anna",
            "--play",
            "--join",
            "0.0.0.0:7777",
        ]
        .map(String::from)
        .to_vec();
        let join = Endpoint::Join("127.0.0.1:7777".parse().unwrap())
            .join_args()
            .unwrap();
        assert_eq!(
            restart_args(&args, "sare-rp-town", &join),
            [
                "--mods-dir",
                "mods",
                "--name",
                "Anna",
                "--play",
                "--content",
                "sare-rp-town",
                "--join",
                "127.0.0.1:7777",
            ]
            .map(String::from)
        );
        assert!(Endpoint::Host("127.0.0.1:7777".parse().unwrap())
            .join_args()
            .is_none());
    }
}
