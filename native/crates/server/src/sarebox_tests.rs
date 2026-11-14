//! SARE Box end to end: the shipped resources, real client sockets, and the
//! client scripts running in the same sandbox the game uses.
use super::*;
use sa_lua::client::{Host, Output, PlayerView, View};

/// Every test here uses this player secret (it is set once per process).
pub const SECRET: &str = "sarebox-test-player";

pub struct Player {
    pub client: sa_net::Session,
    pub host: Host,
    pub pose: sa_net::Pose,
    /// Where client scripts or the server (SetEntityCoords) moved the player.
    pub teleports: Vec<[f32; 3]>,
    /// Payloads of the `chat:addMessage` events this player received.
    pub chat: Vec<String>,
    /// Every event the server sent this player: (name, payload).
    pub events: Vec<(String, String)>,
}
impl Player {
    /// San Andreas world coordinates, as `GetEntityCoords` reports them.
    /// Moves the player now; see `Sarebox::settle` to wait until the server
    /// has seen it.
    pub fn place(&mut self, [x, y, z]: [f32; 3], driving: bool) {
        self.pose.position = [x - 2500.0, z, -1670.0 - y];
        self.pose.driving = driving;
        self.pose.vehicle = driving.then(|| sa_net::VehiclePose {
            position: self.pose.position,
            ..Default::default()
        });
    }
    pub fn text(&self) -> String {
        self.host.ui().text.clone().unwrap_or_default()
    }
    pub fn command(&mut self, line: &str) {
        // As the game does: client commands first, then the server.
        if !self.host.command(line) {
            let payload = Json::Array(vec![line.into()]).to_string();
            self.client
                .trigger(None, "__cfx_internal:commandFallback", &payload)
                .unwrap();
        }
    }
}

pub struct Sarebox {
    pub root: PathBuf,
    pub shared: Shared,
    pub console: Console,
    pub session: Rc<sa_net::Session>,
    known: BTreeMap<u32, String>,
}
impl Sarebox {
    /// A SARE Box server; `prepare` can add files before it starts.
    pub fn boot(extra: &[&str], prepare: impl FnOnce(&Path)) -> Self {
        Self::boot_with(
            "sarebox",
            &[
                "ensure mapmanager",
                "ensure chat",
                "ensure spawnmanager",
                "ensure sare-map-grove",
                "ensure [sarebox]",
            ],
            extra,
            prepare,
        )
    }
    /// A server from any template, starting `resources` (ensure lines).
    pub fn boot_with(
        template: &str,
        resources: &[&str],
        extra: &[&str],
        prepare: impl FnOnce(&Path),
    ) -> Self {
        sa_net::set_identity_secret(SECRET);
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let unique = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("sare-box-{}-{unique}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        template::create(&root, template).unwrap();
        prepare(&root);
        let mut lines = vec!["endpoint_add_tcp 127.0.0.1:0"];
        lines.extend(resources);
        lines.push("add_ace group.admin command allow");
        lines.extend(extra);
        let commands = lines.iter().map(|l| cfg::parse_line(l).remove(0)).collect();
        let Server {
            shared,
            console,
            session,
            publication: _,
        } = boot(commands, root.join("resources")).unwrap();
        shared.borrow_mut().capture = Some(String::new());
        Self {
            root,
            shared,
            console,
            session,
            known: BTreeMap::new(),
        }
    }
    /// Console output since the last call.
    pub fn output(&self) -> String {
        self.shared
            .borrow_mut()
            .capture
            .replace(String::new())
            .unwrap_or_default()
    }
    pub fn run(&mut self, line: &str) {
        self.console.execute_line(0, line);
    }
    /// One server and client frame for every player.
    pub fn step(&mut self, players: &mut [&mut Player]) {
        let views: Vec<PlayerView> = players
            .iter()
            .filter_map(|p| {
                let id = p.client.update(p.pose)?.local_id;
                Some(PlayerView {
                    id,
                    name: "Tester".into(),
                    position: natives::world_position(&p.pose),
                    heading: 0.0,
                    in_vehicle: p.pose.driving,
                })
            })
            .collect();
        pump(
            &self.shared,
            &mut self.console,
            &self.session,
            &mut self.known,
        )
        .unwrap();
        for player in players.iter_mut() {
            for event in player.client.events() {
                player
                    .events
                    .push((event.name.clone(), event.payload.clone()));
                if event.name == "chat:addMessage" {
                    player.chat.push(event.payload.clone());
                }
                // Server SetEntityCoords on a player: the game moves itself.
                if event.name == "__sare:setCoords" {
                    if let Ok(Json::Array(v)) = serde_json::from_str::<Json>(&event.payload) {
                        let at = |i: usize| v.get(i).and_then(Json::as_f64).unwrap_or(0.0) as f32;
                        player.teleports.push([at(0), at(1), at(2)]);
                    }
                }
                player.host.handle_event(&event.name, &event.payload);
            }
            for sha in player.host.take_requests() {
                player.client.request_script(&sha);
            }
            for (sha, bytes) in player.client.take_scripts() {
                player.host.load(&sha, &bytes).unwrap();
            }
            let local_id = player.client.update(player.pose).map_or(0, |r| r.local_id);
            player.host.tick(View {
                local_id,
                players: views.clone(),
            });
            for output in player.host.take_outputs() {
                match output {
                    Output::ServerEvent { name, payload } => {
                        player.client.trigger(None, &name, &payload).unwrap()
                    }
                    Output::Console(line) => assert!(!line.contains("ERROR"), "{line}"),
                    Output::Teleport(position) => player.teleports.push(position),
                    Output::Heading(_) => {}
                }
            }
        }
        thread::sleep(Duration::from_millis(5));
    }
    pub fn until(
        &mut self,
        players: &mut [&mut Player],
        what: &str,
        mut done: impl FnMut(&Self, &[&mut Player]) -> bool,
    ) {
        let start = Instant::now();
        while !done(self, players) {
            self.step(players);
            assert!(
                start.elapsed() < Duration::from_secs(8),
                "timed out waiting for {what}; texts {:?}",
                players.iter().map(|p| p.text()).collect::<Vec<_>>()
            );
        }
    }
    /// Wait until the server sees the player's current pose.
    pub fn settle(&mut self, player: &mut Player) {
        let id = self.id(player);
        let pose = player.pose;
        self.until(&mut [player], "the server to see the pose", |s, _| {
            s.shared.borrow().peers.iter().any(|p| {
                p.id == id && p.pose.position == pose.position && p.pose.driving == pose.driving
            })
        });
    }
    /// Run frames for a while, for checks that something does not happen.
    pub fn idle(&mut self, players: &mut [&mut Player], frames: usize) {
        for _ in 0..frames {
            self.step(players);
        }
    }
    pub fn join(&mut self) -> Player {
        let mut player = Player {
            client: sa_net::Session::join(self.session.address, "Tester").unwrap(),
            host: Host::new(),
            pose: sa_net::Pose::default(),
            teleports: Vec::new(),
            chat: Vec::new(),
            events: Vec::new(),
        };
        self.until(&mut [&mut player], "the character to load", |_, p| {
            p[0].host.ui().text.is_some()
        });
        player
    }
    pub fn leave(&mut self, player: Player) {
        let id = player.client.update(player.pose).map_or(0, |r| r.local_id);
        drop(player);
        self.until(&mut [], "the player to leave", |s, _| {
            !s.shared.borrow().peers.iter().any(|p| p.id == id)
        });
    }
    pub fn id(&self, player: &Player) -> u32 {
        player.client.update(player.pose).unwrap().local_id
    }
    pub fn kvp(&self) -> Json {
        script::flush_kvp(&self.shared, true);
        let text = std::fs::read_to_string(self.root.join("kvp/sarebox_core.json")).unwrap();
        serde_json::from_str(&text).unwrap()
    }
}
impl Drop for Sarebox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn cash(player: &Player) -> String {
    player.text().split("  |").next().unwrap_or("").to_string()
}

#[test]
fn money_is_validated_saved_atomically_and_survives_restarts_and_reconnects() {
    let mut server = Sarebox::boot(&[], |_| {});
    let mut player = server.join();
    let id = server.id(&player);
    let license = server.session.identity(id).unwrap();
    server.run(&format!(
        "add_principal identifier.license:{license} group.admin"
    ));
    // Nonsense amounts change nothing; then one valid change.
    for amount in ["nan", "inf", "1e400", "-5", "1.5", "0", "abc", "2000000000"] {
        player.command(&format!("/givemoney {id} cash {amount}"));
    }
    player.command(&format!("/setmoney {id} bank -1"));
    player.command(&format!("/givemoney {id} cash 25"));
    server.until(&mut [&mut player], "the valid payment", |_, p| {
        cash(p[0]) == "$525"
    });
    server.idle(&mut [&mut player], 40);
    assert_eq!(cash(&player), "$525");
    assert!(player.text().contains("Bank $5000"), "{}", player.text());
    assert!(server.output().contains("[admin]"));

    // Within about a second the change is on disk without any shutdown: an
    // unexpected end of the process would not lose it.
    let file = server.root.join("kvp/sarebox_core.json");
    server.until(&mut [&mut player], "the KVP file", |_, _| {
        std::fs::read_to_string(&file).is_ok_and(|t| t.contains("\\\"cash\\\":525"))
    });

    // Restarting the core twice keeps the character and one HUD, closes the
    // menu it had open, and the admin resource that depends on it is
    // restarted with it and keeps working.
    player.command("/playermenu");
    assert!(player.host.ui().context.is_some());
    for _ in 0..2 {
        server.run("restart sarebox_core");
    }
    assert!(server.shared.borrow().resources["sarebox_admin"].started);
    // Times out if the old menu stays open.
    server.until(&mut [&mut player], "restarted UI", |_, p| {
        let ui = p[0].host.ui();
        p[0].host.resources().iter().any(|r| r == "sarebox_core")
            && ui.text.is_some()
            && ui.context.is_none()
    });
    player.command(&format!("/givemoney {id} cash 5"));
    server.until(&mut [&mut player], "money after restarts", |_, p| {
        cash(p[0]) == "$530"
    });
    server.idle(&mut [&mut player], 40);
    assert_eq!(cash(&player), "$530");
    assert!(
        !player.text().contains("\n\n"),
        "one HUD: {}",
        player.text()
    );
    let paid = player
        .host
        .ui()
        .notifications
        .iter()
        .filter(|n| n.description == "+$5")
        .count();
    assert_eq!(paid, 1, "one handler per event after restarts");
    let output = server.output();
    assert!(!output.contains("SCRIPT ERROR"), "{output}");

    // Leaving and coming back: same character, no extra start money.
    server.leave(player);
    let player = server.join();
    assert_eq!(cash(&player), "$530");
    let saved = server.kvp();
    let characters = saved
        .as_object()
        .unwrap()
        .keys()
        .filter(|k| k.starts_with("player:"))
        .count();
    assert_eq!(characters, 1, "{saved}");
}

#[test]
fn old_characters_move_to_signed_licenses_only_with_admin_approval() {
    let salt = "old-server-salt";
    let legacy = sa_net::resources::hash(format!("sare-identity\n{SECRET}\n{salt}").as_bytes());
    let mut server = Sarebox::boot(&[], |root| {
        std::fs::write(root.join("identity.salt"), salt).unwrap();
        let old = serde_json::json!({
            // Saves use full identifiers as keys: license:license:<hex>.
            format!("license:license:{legacy}"): "OLDCHAR1",
            "player:OLDCHAR1": serde_json::json!({
                "citizenid": "OLDCHAR1", "license": format!("license:{legacy}"), "name": "Old Me",
                "money": { "cash": 4242, "bank": 10 },
                "job": { "name": "taxi", "grade": { "level": 1 } }, "metadata": {}
            }).to_string(),
        });
        std::fs::create_dir_all(root.join("kvp")).unwrap();
        std::fs::write(root.join("kvp/sarebox_core.json"), old.to_string()).unwrap();
    });
    let mut player = server.join();
    let id = server.id(&player);
    // Not loaded yet: a guest, and the console names the claim.
    assert_eq!(cash(&player), "$500");
    assert!(server
        .output()
        .contains("claims the old character OLDCHAR1"));
    // The old license grants nothing, even if an admin config still names it.
    server.run(&format!(
        "add_principal identifier.legacy:{legacy} group.admin"
    ));
    server.run(&format!(
        "add_principal identifier.license:{legacy} group.admin"
    ));
    player.command(&format!("/givemoney {id} cash 1"));
    server.until(&mut [&mut player], "access denied", |s, _| {
        s.shared
            .borrow()
            .capture
            .as_ref()
            .is_some_and(|c| c.contains("Access denied"))
    });

    server.run(&format!("sarebox_migrate {id}"));
    server.until(&mut [&mut player], "the old character", |_, p| {
        cash(p[0]) == "$4242"
    });
    assert!(player.text().contains("Taxi - Driver"));
    let license = server.session.identity(id).unwrap();
    let saved = server.kvp();
    assert_eq!(saved[format!("license:license:{license}")], "OLDCHAR1");
    assert!(saved.get(format!("license:license:{legacy}")).is_none());

    // Next time it loads directly, and nobody can claim it again.
    server.leave(player);
    let player = server.join();
    assert_eq!(cash(&player), "$4242");
    assert!(!server.output().contains("claims"));
}

#[test]
fn rejected_or_disabled_migration_gives_a_new_character() {
    let salt = "old-server-salt";
    let legacy = sa_net::resources::hash(format!("sare-identity\n{SECRET}\n{salt}").as_bytes());
    let prepare = |root: &Path| {
        std::fs::write(root.join("identity.salt"), salt).unwrap();
        let old = serde_json::json!({
            format!("license:license:{legacy}"): "OLDCHAR1",
            "player:OLDCHAR1": "{\"citizenid\":\"OLDCHAR1\",\"money\":{\"cash\":4242,\"bank\":1}}",
        });
        std::fs::create_dir_all(root.join("kvp")).unwrap();
        std::fs::write(root.join("kvp/sarebox_core.json"), old.to_string()).unwrap();
    };
    let mut server = Sarebox::boot(&[], prepare);
    let mut player = server.join();
    let id = server.id(&player);
    server.run(&format!("sarebox_migrate {id} reject"));
    server.until(&mut [&mut player], "a saved new character", |s, _| {
        let license = s.session.identity(id).unwrap();
        s.kvp().get(format!("license:license:{license}")).is_some()
    });
    assert_eq!(cash(&player), "$500");
    // The old character stays untouched for a later decision, and an admin
    // can still give it back over the new one.
    assert_eq!(
        server.kvp()[format!("license:license:{legacy}")],
        "OLDCHAR1"
    );
    server.run(&format!("sarebox_migrate {id}"));
    server.until(&mut [&mut player], "the old character", |_, p| {
        cash(p[0]) == "$4242"
    });
    drop(player);
    drop(server);

    let mut server = Sarebox::boot(&["set sarebox_migration off"], prepare);
    let player = server.join();
    assert_eq!(cash(&player), "$500");
    assert!(!server.output().contains("claims"));
}

/// One dispatch point 370 m north of the default position (2500, -1670),
/// with limits that keep the test short.
const TAXI_CONFIG: &str = r#"
TaxiConfig = {
  Job = 'taxi', Account = 'cash',
  Fare = { Base = 40, PerMeter = 0.12, Max = 450 },
  Radius = 12.0, MinDistance = 250.0, MaxDistance = 3000.0,
  MaxSpeed = 100.0, TimeLimit = 600, Cooldown = 2,
  Points = { { label = 'North Point', x = 2500.0, y = -1300.0, z = 0.0 } },
}
"#;
const START: [f32; 3] = [2500.0, -1670.0, 0.0];
const POINT: [f32; 3] = [2500.0, -1300.0, 0.0];

fn taxi_server() -> Sarebox {
    Sarebox::boot(&[], |root| {
        std::fs::write(
            root.join("resources/[sarebox]/sarebox_taxi/config.lua"),
            TAXI_CONFIG,
        )
        .unwrap();
    })
}
fn notified(player: &Player, text: &str) -> bool {
    player
        .host
        .ui()
        .notifications
        .iter()
        .any(|n| n.description.contains(text))
}
/// Pick a taxi menu option by title, as a player clicking it would.
fn taxi_menu(server: &mut Sarebox, player: &mut Player, title: &str) {
    player.command("/taxi");
    let index = {
        let ui = player.host.ui();
        let menu = ui.context.as_ref().expect("taxi menu");
        menu.options
            .iter()
            .position(|o| o.title == title)
            .unwrap_or_else(|| panic!("no option {title}: {menu:?}"))
    };
    player.host.select_context(index);
    server.step(&mut [player]);
}
/// Drive in a straight line at `speed` m/s until `to`.
fn drive(server: &mut Sarebox, player: &mut Player, to: [f32; 3], speed: f32) {
    let mut at = natives::world_position(&player.pose);
    let mut last = Instant::now();
    let start = Instant::now();
    loop {
        let (dx, dy) = (to[0] - at[0], to[1] - at[1]);
        let left = (dx * dx + dy * dy).sqrt();
        if left < 1.0 {
            break;
        }
        let step = (speed * last.elapsed().as_secs_f32()).min(left);
        last = Instant::now();
        at = [at[0] + dx / left * step, at[1] + dy / left * step, to[2]];
        player.place(at, true);
        server.step(&mut [player]);
        assert!(start.elapsed() < Duration::from_secs(20), "drive timed out");
    }
    server.settle(player);
}

#[test]
fn taxi_runs_are_started_checked_and_paid_by_the_server_once() {
    let mut server = taxi_server();
    assert!(server.shared.borrow().resources["sarebox_taxi"].started);
    let mut player = server.join();
    player.place(START, true);
    server.settle(&mut player);

    // Only taxi drivers get runs; signing up is checked on the server.
    taxi_menu(&mut server, &mut player, "Take a dispatch");
    server.until(&mut [&mut player], "the refusal", |_, p| {
        notified(p[0], "Only on-duty taxi drivers")
    });
    taxi_menu(&mut server, &mut player, "Sign up as a taxi driver");
    server.until(&mut [&mut player], "the taxi job", |_, p| {
        p[0].text().contains("Taxi - Recruit")
    });
    // Not in a car: no run.
    player.place(START, false);
    server.settle(&mut player);
    taxi_menu(&mut server, &mut player, "Take a dispatch");
    server.until(&mut [&mut player], "the car check", |_, p| {
        notified(p[0], "Get in a car first")
    });

    // A run: the server names the point and the fare (40 + 370 * 0.12).
    player.place(START, true);
    server.settle(&mut player);
    taxi_menu(&mut server, &mut player, "Take a dispatch");
    server.until(&mut [&mut player], "the route", |_, p| {
        p[0].text().contains("Taxi: drive to North Point")
    });
    assert!(
        player.text().contains("370 m north  |  fare $84"),
        "{}",
        player.text()
    );
    // A yellow blip on the dispatch point, with the GPS route to it.
    let blips = player.host.blips();
    assert_eq!(blips.len(), 1);
    assert_eq!(blips[0].label.as_deref(), Some("Taxi: North Point"));
    assert!(blips[0].route && blips[0].colour == 5);
    assert!((blips[0].position[1] - POINT[1]).abs() < 1.0);

    // Made-up events cannot pay, and teleporting there ends the run unpaid.
    for (name, payload) in [
        ("sarebox_taxi:server:finish", "[9999]"),
        ("sarebox_taxi:server:arrived", "[]"),
        ("sarebox:server:moneyChanged", "[1, \"cash\", 9999]"),
    ] {
        player.client.trigger(None, name, payload).unwrap();
    }
    player.place(POINT, true);
    server.until(&mut [&mut player], "the teleport check", |_, p| {
        notified(p[0], "position jumped")
    });
    assert!(!player.text().contains("Taxi:"));
    assert_eq!(cash(&player), "$500");
    assert!(player.host.blips().is_empty());

    // Driving there in time is paid once, and the menu no longer offers a
    // cancel for a finished run.
    player.place(START, true);
    server.settle(&mut player);
    taxi_menu(&mut server, &mut player, "Take a dispatch");
    server.until(&mut [&mut player], "the route", |_, p| {
        p[0].text().contains("Taxi: drive to")
    });
    drive(&mut server, &mut player, POINT, 85.0);
    // Cash and the notification are separate messages, so wait for both.
    server.until(&mut [&mut player], "the fare", |_, p| {
        cash(p[0]) == "$584" && notified(p[0], "Paid $84")
    });
    taxi_menu(&mut server, &mut player, "Take a dispatch");
    server.until(&mut [&mut player], "the cooldown", |_, p| {
        notified(p[0], "nothing for you yet")
    });
    server.idle(&mut [&mut player], 100);
    assert_eq!(cash(&player), "$584");

    // Too fast for the distance: not paid.
    drive(&mut server, &mut player, START, 85.0);
    thread::sleep(Duration::from_secs(2));
    taxi_menu(&mut server, &mut player, "Take a dispatch");
    server.until(&mut [&mut player], "the route", |_, p| {
        p[0].text().contains("Taxi: drive to")
    });
    drive(&mut server, &mut player, POINT, 190.0);
    server.until(&mut [&mut player], "the speed check", |_, p| {
        notified(p[0], "impossibly fast")
    });
    assert_eq!(cash(&player), "$584");

    // Going off duty cancels the run.
    player.place(START, true);
    server.settle(&mut player);
    taxi_menu(&mut server, &mut player, "Take a dispatch");
    server.until(&mut [&mut player], "the route", |_, p| {
        p[0].text().contains("Taxi: drive to")
    });
    player
        .client
        .trigger(None, "sarebox:server:toggleDuty", "[]")
        .unwrap();
    server.until(&mut [&mut player], "the duty check", |_, p| {
        notified(p[0], "no longer an on-duty taxi driver")
    });
    assert!(!player.text().contains("Taxi:"));
    player
        .client
        .trigger(None, "sarebox:server:toggleDuty", "[]")
        .unwrap();

    // Restarting the resource mid-run cancels it and clears the directions.
    taxi_menu(&mut server, &mut player, "Take a dispatch");
    server.until(&mut [&mut player], "the route", |_, p| {
        p[0].text().contains("Taxi: drive to")
    });
    server.run("restart sarebox_taxi");
    server.until(&mut [&mut player], "the restart", |_, p| {
        notified(p[0], "Taxi dispatch restarted") && !p[0].text().contains("Taxi:")
    });
    drive(&mut server, &mut player, POINT, 85.0);
    server.idle(&mut [&mut player], 100);
    assert_eq!(cash(&player), "$584");

    // Leaving mid-run ends it: nothing is paid on return.
    drive(&mut server, &mut player, START, 85.0);
    taxi_menu(&mut server, &mut player, "Take a dispatch");
    server.until(&mut [&mut player], "the route", |_, p| {
        p[0].text().contains("Taxi: drive to")
    });
    server.leave(player);
    let mut player = server.join();
    player.place(POINT, true);
    server.idle(&mut [&mut player], 100);
    assert_eq!(cash(&player), "$584");
    assert!(!player.text().contains("Taxi:"));
    // The fare was saved with the character.
    let saved = server.kvp().to_string();
    assert!(saved.contains("\\\"cash\\\":584"), "{saved}");
}
