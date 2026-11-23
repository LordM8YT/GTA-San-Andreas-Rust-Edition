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
use crate::script::{self, out, Shared};
use sa_scene::world_layout::{Layout, NodeId};
use sa_scene::world_logic::{wire, Happened, PlayerState, Shared as WorldState, World};
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
    names: BTreeMap<NodeId, String>,
    /// Shared state per routing bucket, created when someone first plays there.
    pub buckets: BTreeMap<u32, WorldState>,
    players: BTreeMap<u32, Player>,
}

struct Player {
    state: PlayerState,
    bucket: u32,
    /// Bucket and revision of the last snapshot sent.
    sent: Option<(u32, u64)>,
    last_step: Instant,
    last_use: Option<Instant>,
    refused: u32,
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
        }
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
        let world = &self.world;
        let state = self.buckets.entry(bucket).or_insert_with(|| world.shared());
        world.visible(state, node)
    }
    /// Players the world keeps state for.
    #[cfg(test)]
    pub fn player_count(&self) -> usize {
        self.players.len()
    }
    /// The first object with this name in the layout.
    pub fn find(&self, name: &str) -> Option<NodeId> {
        self.names
            .iter()
            .find(|(_, n)| n.as_str() == name)
            .map(|(id, _)| *id)
    }
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
                    if game.world.has_gameplay() {
                        ""
                    } else {
                        " (it has no gameplay objects yet)"
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
    names: &BTreeMap<NodeId, String>,
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
    let name = |node: &NodeId| Json::from(names.get(node).cloned().unwrap_or_default());
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
    game.players
        .retain(|id, _| peers.iter().any(|peer| peer.id == *id));
    let mut deliveries = Vec::new();
    for peer in &peers {
        let bucket = session.as_ref().map_or(0, |s| s.routing_bucket(peer.id));
        let player = game.players.entry(peer.id).or_insert_with(|| Player {
            state: PlayerState::new(),
            bucket,
            sent: None,
            last_step: now,
            last_use: None,
            refused: 0,
        });
        if player.bucket != bucket {
            // Another instance: its zones are new, its state is another.
            player.bucket = bucket;
            player.state = PlayerState::new();
            player.sent = None;
        }
        let dt = (now - player.last_step).as_secs_f32().min(1.0);
        player.last_step = now;
        let world = &game.world;
        let state = game.buckets.entry(bucket).or_insert_with(|| world.shared());
        let effects = world.step(state, &mut player.state, feet(&peer.pose), dt);
        deliveries.push((peer.id, effects, player.state.take_events()));
    }
    // Snapshots for whoever has not seen their bucket's latest state.
    let mut snapshots = Vec::new();
    for (id, player) in &mut game.players {
        let world = &game.world;
        let state = game
            .buckets
            .entry(player.bucket)
            .or_insert_with(|| world.shared());
        let current = (player.bucket, state.revision());
        if player.sent != Some(current) {
            player.sent = Some(current);
            snapshots.push((*id, state.snapshot()));
        }
    }
    let names = game.names.clone();
    // Handlers may call the world natives: give the game back first.
    shared.borrow_mut().world = Some(game);
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
        if player
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
                Some(effects) => result = Some((effects, player.state.take_events())),
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
    let names = game.names.clone();
    shared.borrow_mut().world = Some(game);
    if let Some((effects, events)) = result {
        deliver(shared, source, &effects, events, &names);
    }
    true
}
