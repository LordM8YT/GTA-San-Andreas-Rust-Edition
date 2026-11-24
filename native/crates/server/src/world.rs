//! Build Mode gameplay, run by the server for everyone in a content pack
//! world (`sets content_pack <id>`).
//!
//! The server is the authority: it loads the pack's `layout.json` itself,
//! checks every player's zones against the poses it receives, runs the
//! actions and keeps the shared state (objects shown or hidden, the hour,
//! the weather) per routing bucket, so each instance has its own doors.
//! Players get that state as a snapshot when it changes, when they arrive
//! and when they change bucket, plus their own personal effects (messages,
//! teleports). The interact key reaches the server as a request
//! (`__sare:worldUse`), which is checked against the server's view of the
//! player's position and rate limited; clients never send effects or state.
//!
//! Resources can react with the events `sare:zoneEnter`, `sare:zoneLeave`,
//! `sare:objectUsed`, `sare:checkpoint` and `sare:raceFinished` (source =
//! the player), and change the world with the natives in `natives.rs`
//! (`SetWorldObjectVisible`, `SetWorldHour`, `SetWorldWeather`, ...).
//! The rules themselves are `sa_scene::world_logic`, shared with the game.
//!
//! The world's own Lua scripts (Build Mode's world scripts) run here too,
//! once per instance, in the sandbox of `sa_lua::world`: they get the same
//! moments, and what they change is shared or sent like an action's
//! effects. Their output goes to the console.
use crate::script::{self, out, Shared};
use sa_scene::world_layout::{Layout, NodeId};
use sa_scene::world_logic::{wire, Happened, PlayerState, Shared as WorldState, World};
use sa_scene::world_scripts::{self as ws, Scripts, Who};
use serde_json::Value as Json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How far (metres) outside an interaction zone a use is still accepted:
/// the server sees positions a little late.
pub const USE_SLACK: f32 = 2.0;
/// Shortest time between two uses by one player.
pub const USE_INTERVAL: Duration = Duration::from_millis(250);
/// Refusals logged per player, so a misbehaving client cannot flood the log.
const LOGGED_REFUSALS: u32 = 3;

pub struct Game {
    pub world: World,
    /// Object names in layout order.
    names: Vec<(NodeId, String)>,
    /// Shared state per routing bucket, created when someone first plays there.
    pub buckets: BTreeMap<u32, WorldState>,
    players: BTreeMap<u32, Player>,
    warned_size: bool,
    /// The layout, for starting an instance's scripts.
    layout: Layout,
    /// Scripts per instance, started with the instance (None: it has none).
    scripts: BTreeMap<u32, Option<Scripts>>,
    /// What happened per instance since the scripts last ran.
    script_events: BTreeMap<u32, Vec<sa_lua::world::Event>>,
    last_scripts: Option<Instant>,
    /// Since when an instance with running scripts has had no players.
    empty_since: BTreeMap<u32, Instant>,
}

/// How long an instance's scripts keep running with nobody there.
const IDLE_SCRIPTS: Duration = Duration::from_secs(30);

struct Player {
    state: PlayerState,
    bucket: u32,
    /// Bucket and revision of the last snapshot sent.
    sent: Option<(u32, u64)>,
    last_step: Instant,
    last_use: Option<Instant>,
    refused: u32,
    /// A teleport the player's game has not reported yet: where to, and
    /// since when. Zones wait until the server sees the player arrive.
    arriving: Option<([f32; 3], Instant)>,
}

impl Game {
    pub fn new(layout: &Layout) -> Self {
        Self {
            world: World::new(layout),
            names: layout
                .nodes
                .iter()
                .map(|n| (n.id, n.name.clone()))
                .collect(),
            buckets: BTreeMap::new(),
            players: BTreeMap::new(),
            warned_size: false,
            layout: layout.clone(),
            scripts: BTreeMap::new(),
            script_events: BTreeMap::new(),
            last_scripts: None,
            empty_since: BTreeMap::new(),
        }
    }
    /// Whether the world has scripts to run.
    pub fn has_scripts(&self) -> bool {
        !sa_scene::world_logic::script_sources(&self.layout).is_empty()
    }
    fn queue(&mut self, bucket: u32, event: sa_lua::world::Event) {
        if self.scripts.get(&bucket).is_some_and(Option::is_none) {
            return;
        }
        self.script_events.entry(bucket).or_default().push(event);
    }
    /// The shared state of `bucket`, created from the world on first use.
    pub fn bucket(&mut self, bucket: u32) -> &mut WorldState {
        let world = &self.world;
        self.buckets.entry(bucket).or_insert_with(|| world.shared())
    }
    /// Show or hide an object in `bucket`; false when there is no such object.
    pub fn set_visible(&mut self, bucket: u32, node: NodeId, visible: bool) -> bool {
        let world = &self.world;
        self.buckets
            .entry(bucket)
            .or_insert_with(|| world.shared())
            .set_visible(world, node, visible)
    }
    pub fn visible(&mut self, bucket: u32, node: NodeId) -> bool {
        // Asking makes no state: an instance nobody played is as placed.
        match self.buckets.get(&bucket) {
            Some(state) => self.world.visible(state, node),
            None => self.world.visible(&self.world.shared(), node),
        }
    }
    /// Put instance `bucket` (or every instance, for None) back to how the
    /// world starts; players there get the new state, and their "only the
    /// first time" triggers work again. How many instances had state.
    pub fn reset(&mut self, bucket: Option<u32>) -> usize {
        let world = &self.world;
        let chosen = |b: u32| bucket.is_none_or(|only| b == only);
        let mut count = 0;
        for (_, state) in self.buckets.iter_mut().filter(|(b, _)| chosen(**b)) {
            state.reset(world);
            count += 1;
        }
        // Scripts start again too; the players there join them anew.
        self.scripts.retain(|b, _| !chosen(*b));
        self.script_events.retain(|b, _| !chosen(*b));
        let rejoin: Vec<(u32, u32)> = self
            .players
            .iter()
            .filter(|(_, p)| chosen(p.bucket))
            .map(|(id, p)| (p.bucket, *id))
            .collect();
        for (bucket, id) in rejoin {
            self.queue(bucket, ws::joined(id));
        }
        // Their once-only triggers work again for the players there.
        for player in self.players.values_mut().filter(|p| chosen(p.bucket)) {
            player.state.world_reset();
        }
        count
    }
    /// Players the world keeps state for.
    #[cfg(test)]
    pub fn player_count(&self) -> usize {
        self.players.len()
    }
    /// The first object with this name in the layout (outliner order).
    pub fn find(&self, name: &str) -> Option<NodeId> {
        self.names
            .iter()
            .find(|(_, n)| n.as_str() == name)
            .map(|(id, _)| *id)
    }
    /// Whether the layout has this object.
    pub fn has_object(&self, node: NodeId) -> bool {
        self.world.has_object(node)
    }
}

/// How close (metres) to a teleport's destination counts as arrived.
const ARRIVED: f32 = 3.0;
/// Longest wait for a teleported player's game to report the new place.
const ARRIVAL_WAIT: Duration = Duration::from_secs(3);

/// Where the last teleport among `effects` leads.
fn teleport(effects: &[sa_scene::world_logic::Effect]) -> Option<[f32; 3]> {
    effects.iter().rev().find_map(|e| match e {
        sa_scene::world_logic::Effect::Teleport(pose) => Some(pose.position),
        _ => None,
    })
}

/// Find content pack `id` for the server: `content/` next to server.cfg,
/// then the folders the game itself looks in.
pub fn find_pack(server_root: &Path, id: &str) -> Option<PathBuf> {
    let mut dirs = vec![server_root.join(sa_scene::content_pack::DEFAULT_DIR)];
    dirs.extend(sa_scene::content::pack_dirs());
    dirs.iter()
        .find_map(|dir| sa_scene::content_pack::resolve(id, dir).ok())
}

/// Load the gameplay of content pack `id`, if the server has the pack and
/// it has a layout. Without it, each player's game runs the gameplay alone.
pub fn load(shared: &Shared, server_root: &Path, id: &str) {
    let Some(folder) = find_pack(server_root, id) else {
        out(
            shared,
            format!(
                "World gameplay: content pack {id} is not on this server (put it in content/{id} next to server.cfg); each player's game runs the gameplay alone, unsynchronised"
            ),
        );
        return;
    };
    let layout = sa_scene::content_pack::Manifest::read(&folder).and_then(|manifest| {
        let Some(file) = manifest.world.layout else {
            return Ok(None);
        };
        Layout::read(&sa_scene::content_pack::inside(&folder, &file)?).map(Some)
    });
    match layout {
        Ok(Some(layout)) => {
            let game = Game::new(&layout);
            out(
                shared,
                format!(
                    "World gameplay: {} from {} runs on the server{}",
                    id,
                    folder.display(),
                    match (game.world.has_gameplay(), game.has_scripts()) {
                        (_, true) => " with its scripts",
                        (true, false) => "",
                        (false, false) => " (it has no gameplay objects yet)",
                    }
                ),
            );
            shared.borrow_mut().world = Some(game);
        }
        Ok(None) => {}
        Err(error) => out(
            shared,
            format!("World gameplay of {id} not loaded: {error:#}"),
        ),
    }
}

/// A player's feet in the pack's Blender axes, from the pose the server has.
fn feet(pose: &sa_net::Pose) -> [f32; 3] {
    let [x, y, z] = crate::natives::world_position(pose);
    let anchor = sa_scene::content_pack::ANCHOR;
    [x - anchor[0], y - anchor[1], z]
}

fn send(shared: &Shared, player: u32, name: &str, args: &[Json]) {
    crate::natives::send_client(shared, Some(player), name, args);
}

/// Personal effects to their player, and what happened to Lua.
fn deliver(
    shared: &Shared,
    player: u32,
    effects: &[sa_scene::world_logic::Effect],
    events: Vec<Happened>,
    names: &[(NodeId, String)],
) {
    let personal: Vec<wire::Personal> = effects.iter().filter_map(wire::Personal::of).collect();
    for chunk in personal.chunks(8) {
        send(
            shared,
            player,
            wire::EFFECTS_EVENT,
            &[serde_json::to_value(chunk).unwrap_or_default()],
        );
    }
    let name = |node: &NodeId| {
        Json::from(
            names
                .iter()
                .find(|(id, _)| id == node)
                .map_or("", |(_, n)| n.as_str()),
        )
    };
    for event in events {
        let (event_name, args) = match &event {
            Happened::Entered(node) => ("sare:zoneEnter", vec![Json::from(*node), name(node)]),
            Happened::Left(node) => ("sare:zoneLeave", vec![Json::from(*node), name(node)]),
            Happened::Used(node) => ("sare:objectUsed", vec![Json::from(*node), name(node)]),
            Happened::Checkpoint { race, index, total } => (
                "sare:checkpoint",
                vec![
                    Json::from(race.as_str()),
                    Json::from(index + 1),
                    Json::from(*total),
                ],
            ),
            Happened::Finished { race, seconds } => (
                "sare:raceFinished",
                vec![Json::from(race.as_str()), Json::from(*seconds)],
            ),
        };
        script::dispatch(shared, event_name, player, &args, false);
    }
}

/// Once a server frame: step every player, send what changed.
pub fn tick(shared: &Shared) {
    let Some(mut game) = shared.borrow_mut().world.take() else {
        return;
    };
    let (peers, session) = {
        let state = shared.borrow();
        (state.peers.clone(), state.session.clone())
    };
    let now = Instant::now();
    let gone: Vec<(u32, u32)> = game
        .players
        .iter()
        .filter(|(id, _)| !peers.iter().any(|peer| peer.id == **id))
        .map(|(id, p)| (p.bucket, *id))
        .collect();
    for (bucket, id) in gone {
        game.queue(bucket, ws::left(id));
    }
    game.players
        .retain(|id, _| peers.iter().any(|peer| peer.id == *id));
    let mut deliveries = Vec::new();
    for peer in &peers {
        let bucket = session.as_ref().map_or(0, |s| s.routing_bucket(peer.id));
        if !game.players.contains_key(&peer.id) {
            game.queue(bucket, ws::joined(peer.id));
        }
        let player = game.players.entry(peer.id).or_insert_with(|| Player {
            state: PlayerState::new(),
            bucket,
            sent: None,
            last_step: now,
            last_use: None,
            refused: 0,
            arriving: None,
        });
        if player.bucket != bucket {
            // Another instance: its zones are new, its state is another.
            let old = std::mem::replace(&mut player.bucket, bucket);
            player.state = PlayerState::new();
            player.sent = None;
            player.arriving = None;
            game.queue(old, ws::left(peer.id));
            game.queue(bucket, ws::joined(peer.id));
        }
        let Some(player) = game.players.get_mut(&peer.id) else {
            continue;
        };
        let dt = (now - player.last_step).as_secs_f32().min(1.0);
        player.last_step = now;
        // Not spawned yet: the pose is still the default one.
        if peer.pose == sa_net::Pose::default() {
            continue;
        }
        let here = feet(&peer.pose);
        if let Some((to, since)) = player.arriving {
            let gap: f32 = here.iter().zip(to).map(|(a, b)| (a - b) * (a - b)).sum();
            let there = gap.sqrt() < ARRIVED;
            if !there && now - since < ARRIVAL_WAIT {
                // Zones wait for the player, race clocks do not.
                player.state.wait(dt);
                continue;
            }
            // Arrived (or gave up waiting): take the zones here quietly.
            player.arriving = None;
            player.state.moved();
        }
        let world = &game.world;
        let state = game.buckets.entry(bucket).or_insert_with(|| world.shared());
        let effects = world.step(state, &mut player.state, here, dt);
        if let Some(to) = teleport(&effects) {
            player.arriving = Some((to, now));
        }
        let events = player.state.take_events();
        for event in &events {
            game.queue(bucket, ws::event(peer.id, event.clone()));
        }
        deliveries.push((peer.id, effects, events));
    }
    let mut lines = Vec::new();
    run_scripts(&mut game, &peers, now, &mut deliveries, &mut lines);
    // Snapshots for whoever has not seen their bucket's latest state.
    let mut snapshots = Vec::new();
    let mut too_big = None;
    for (id, player) in &mut game.players {
        let world = &game.world;
        let state = game
            .buckets
            .entry(player.bucket)
            .or_insert_with(|| world.shared());
        let current = (player.bucket, state.revision());
        if player.sent != Some(current) {
            player.sent = Some(current);
            let snapshot = state.snapshot();
            let size = serde_json::to_string(&snapshot).map_or(0, |s| s.len());
            if size + 2 > sa_net::MAX_EVENT_PAYLOAD && !game.warned_size {
                game.warned_size = true;
                too_big = Some(size);
            }
            snapshots.push((*id, snapshot));
        }
    }
    let names = game.names.clone();
    // Handlers may call the world natives: give the game back first.
    shared.borrow_mut().world = Some(game);
    for line in lines {
        out(shared, line);
    }
    if let Some(size) = too_big {
        out(
            shared,
            format!(
                "World gameplay: the world state is {size} bytes, over the {} byte event limit; too many objects are shown or hidden at once",
                sa_net::MAX_EVENT_PAYLOAD
            ),
        );
    }
    for (id, snapshot) in snapshots {
        send(
            shared,
            id,
            wire::STATE_EVENT,
            &[serde_json::to_value(snapshot).unwrap_or_default()],
        );
    }
    for (id, effects, events) in deliveries {
        if !effects.is_empty() || !events.is_empty() {
            deliver(shared, id, &effects, events, &names);
        }
    }
}

/// Client requests about world gameplay. Consumes every `__sare:world*`
/// event, so a client can never pose as the server; only uses are acted on.
pub fn handle_event(shared: &Shared, source: u32, name: &str, args: &[Json]) -> bool {
    if !name.starts_with("__sare:world") {
        return false;
    }
    if name != wire::USE_EVENT {
        return true;
    }
    let Some(node) = args.first().and_then(Json::as_u64) else {
        return true;
    };
    let pose = shared
        .borrow()
        .peers
        .iter()
        .find(|p| p.id == source)
        .map(|p| p.pose);
    let Some(mut game) = shared.borrow_mut().world.take() else {
        return true;
    };
    let mut result = None;
    let mut refusal = None;
    if let (Some(pose), Some(player)) = (pose, game.players.get_mut(&source)) {
        let now = Instant::now();
        if pose.driving && game.world.on_foot_only(node) {
            // Checked first: a refused driver does not use up the rate limit.
            refusal = Some("on foot only, and the player is driving");
        } else if player
            .last_use
            .is_some_and(|last| now - last < USE_INTERVAL)
        {
            refusal = Some("too fast");
        } else {
            player.last_use = Some(now);
            let world = &game.world;
            let state = game
                .buckets
                .entry(player.bucket)
                .or_insert_with(|| world.shared());
            match world.use_node(state, &mut player.state, node, feet(&pose), USE_SLACK) {
                Some(effects) => {
                    if let Some(to) = teleport(&effects) {
                        player.arriving = Some((to, now));
                    }
                    result = Some((effects, player.state.take_events(), player.bucket))
                }
                None => refusal = Some("not standing in it"),
            }
        }
        if let Some(why) = refusal {
            player.refused += 1;
            if player.refused <= LOGGED_REFUSALS {
                out(
                    shared,
                    format!("World gameplay: refused player {source} using object {node} ({why})"),
                );
            }
        }
    }
    if let Some((_, events, bucket)) = &result {
        for event in events {
            game.queue(*bucket, ws::event(source, event.clone()));
        }
    }
    let names = game.names.clone();
    shared.borrow_mut().world = Some(game);
    if let Some((effects, events, _)) = result {
        deliver(shared, source, &effects, events, &names);
    }
    true
}

/// Run each instance's scripts once: start them with the instance, pass on
/// what happened, and carry out what they ask (shared changes in the
/// instance's state, personal ones as deliveries). Console lines go to
/// `lines`.
fn run_scripts(
    game: &mut Game,
    peers: &[sa_net::Peer],
    now: Instant,
    deliveries: &mut Vec<(u32, Vec<sa_scene::world_logic::Effect>, Vec<Happened>)>,
    lines: &mut Vec<String>,
) {
    let dt = game
        .last_scripts
        .replace(now)
        .map_or(0.0, |last| (now - last).as_secs_f32().min(1.0));
    // Objects scripts move go on moving (clients move them the same way).
    for state in game.buckets.values_mut() {
        state.advance(dt);
    }
    // Instances nobody has been in for a while stop their scripts (the
    // state stays); they start again when someone comes.
    let occupied: std::collections::BTreeSet<u32> =
        game.players.values().map(|p| p.bucket).collect();
    let running: Vec<u32> = game.scripts.keys().copied().collect();
    for bucket in running {
        if occupied.contains(&bucket) {
            game.empty_since.remove(&bucket);
        } else if now - *game.empty_since.entry(bucket).or_insert(now) >= IDLE_SCRIPTS {
            game.empty_since.remove(&bucket);
            game.scripts.remove(&bucket);
            game.script_events.remove(&bucket);
            if bucket != 0 {
                lines.push(format!(
                    "World instance {bucket} is empty: its scripts stopped until someone comes back"
                ));
            }
        }
    }
    // Instances with players start their scripts.
    for bucket in occupied {
        if game.scripts.contains_key(&bucket) {
            continue;
        }
        let state = game.bucket(bucket).clone();
        let started = Scripts::start(&game.layout, &state);
        if started.is_none() {
            game.script_events.remove(&bucket);
        }
        game.scripts.insert(bucket, started);
    }
    let mut scripts = std::mem::take(&mut game.scripts);
    for (bucket, scripts) in scripts.iter_mut() {
        let Some(scripts) = scripts else { continue };
        let players: Vec<ws::PlayerInfo> = peers
            .iter()
            .filter(|peer| {
                game.players
                    .get(&peer.id)
                    .is_some_and(|p| p.bucket == *bucket)
            })
            .map(|peer| ws::PlayerInfo {
                id: peer.id,
                name: peer.name.clone(),
                // Not spawned yet: the pose is still the default one.
                position: (peer.pose != sa_net::Pose::default()).then(|| feet(&peer.pose)),
                driving: peer.pose.driving,
            })
            .collect();
        let events = game.script_events.remove(bucket).unwrap_or_default();
        let world = &game.world;
        let state = game
            .buckets
            .entry(*bucket)
            .or_insert_with(|| world.shared());
        let hour = state.hour().or(state.jump().map(|(_, h)| h));
        let changes = scripts.tick(world, state, dt, hour, players.clone(), events);
        let mut personal: BTreeMap<u32, Vec<sa_scene::world_logic::Effect>> = BTreeMap::new();
        for (who, change) in changes {
            match who {
                Who::World => {
                    // Shared: everyone gets it with the next snapshot.
                    world.apply_script(state, &mut PlayerState::new(), change);
                }
                Who::Everyone => {
                    for player in &players {
                        if let Some(p) = game.players.get_mut(&player.id) {
                            personal
                                .entry(player.id)
                                .or_default()
                                .extend(world.apply_script(state, &mut p.state, change.clone()));
                        }
                    }
                }
                Who::Player(id) => {
                    if let Some(p) = game.players.get_mut(&id).filter(|p| p.bucket == *bucket) {
                        let effects = world.apply_script(state, &mut p.state, change);
                        if let Some(to) = teleport(&effects) {
                            p.arriving = Some((to, now));
                        }
                        personal.entry(id).or_default().extend(effects);
                    }
                }
            }
        }
        for (id, effects) in personal {
            deliveries.push((id, effects, Vec::new()));
        }
        let prefix = if *bucket == 0 {
            String::new()
        } else {
            format!("[instance {bucket}] ")
        };
        for line in scripts.take_output() {
            lines.push(format!("{prefix}{}", ws::describe(&line)));
        }
    }
    // Instances reset meanwhile were dropped from the map; keep the rest.
    for (bucket, running) in scripts {
        game.scripts.entry(bucket).or_insert(running);
    }
}
