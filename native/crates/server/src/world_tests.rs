//! Build Mode gameplay on a real server with several connected players:
//! the server runs `content/gameplay-sample` and is the authority for doors,
//! zones, time, weather and uses (see `world.rs`).
use super::sarebox_tests::{Player, Sarebox};
use super::*;
use sa_scene::world_logic::{wire, Snapshot};

/// A resource that reports gameplay events and drives the world natives.
const TEST_RESOURCE: &str = r#"
AddEventHandler('sare:zoneEnter', function(node, name)
    print(('WORLDTEST enter %s by %d'):format(name, source))
end)
AddEventHandler('sare:objectUsed', function(node, name)
    print(('WORLDTEST used %s by %d'):format(name, source))
end)
RegisterCommand('bucket', function(src, args)
    SetPlayerRoutingBucket(src, tonumber(args[1]))
end)
RegisterCommand('closegate', function()
    print(('WORLDTEST closed %s'):format(tostring(SetWorldObjectVisible('Garden gate', true))))
end)
RegisterCommand('storm', function()
    SetWorldWeather('rain')
    SetWorldHour(3)
    print(('WORLDTEST gate visible %s'):format(tostring(IsWorldObjectVisible(GetWorldObjectByName('Garden gate')))))
end)
"#;

fn server() -> Sarebox {
    Sarebox::boot_with(
        "freeroam",
        &["ensure chat", "ensure world_test"],
        &["sets content_pack gameplay-sample"],
        |root| {
            let sample =
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../content/gameplay-sample");
            let target = root.join("content/gameplay-sample");
            std::fs::create_dir_all(&target).unwrap();
            for file in ["content.json", "layout.json"] {
                std::fs::copy(sample.join(file), target.join(file)).unwrap();
            }
            let resource = root.join("resources/[test]/world_test");
            std::fs::create_dir_all(&resource).unwrap();
            std::fs::write(
                resource.join("fxmanifest.lua"),
                "fx_version 'cerulean'\ngame 'gta5'\nserver_script 'server.lua'\n",
            )
            .unwrap();
            std::fs::write(resource.join("server.lua"), TEST_RESOURCE).unwrap();
        },
    )
}

/// A connected player, once the server has sent them the world state.
fn join(server: &mut Sarebox) -> Player {
    let mut player = Player {
        client: sa_net::Session::join(server.session.address, "Tester").unwrap(),
        host: sa_lua::client::Host::new(),
        pose: sa_net::Pose::default(),
        teleports: Vec::new(),
        chat: Vec::new(),
        events: Vec::new(),
    };
    stand(&mut player, [0.0, -6.0, 0.0]);
    server.until(&mut [&mut player], "the world state", |_, p| {
        snapshot(p[0]).is_some()
    });
    player
}

/// Put a player at a point of the pack (Blender axes).
fn stand(player: &mut Player, [x, y, z]: [f32; 3]) {
    let anchor = sa_scene::content_pack::ANCHOR;
    player.place([anchor[0] + x, anchor[1] + y, z], false);
}

/// The newest world state the player received.
fn snapshot(player: &Player) -> Option<Snapshot> {
    player
        .events
        .iter()
        .rev()
        .find(|(name, _)| name == wire::STATE_EVENT)
        .and_then(|(_, payload)| serde_json::from_str::<(Snapshot,)>(payload).ok())
        .map(|(s,)| s)
}

/// Personal effects the player received, oldest first.
fn personal(player: &Player) -> Vec<wire::Personal> {
    player
        .events
        .iter()
        .filter(|(name, _)| name == wire::EFFECTS_EVENT)
        .filter_map(|(_, payload)| serde_json::from_str::<(Vec<wire::Personal>,)>(payload).ok())
        .flat_map(|(list,)| list)
        .collect()
}
fn messages(player: &Player) -> Vec<String> {
    personal(player)
        .into_iter()
        .filter_map(|p| match p {
            wire::Personal::Message { text, .. } => Some(text),
            _ => None,
        })
        .collect()
}

fn use_object(player: &Player, node: u64) {
    player
        .client
        .trigger(None, wire::USE_EVENT, &format!("[{node}]"))
        .unwrap();
}

fn node(server: &Sarebox, name: &str) -> u64 {
    server
        .shared
        .borrow()
        .world
        .as_ref()
        .expect("the server runs the world")
        .find(name)
        .unwrap()
}

#[test]
fn players_share_one_authoritative_world() {
    let mut server = server();
    assert!(
        server.shared.borrow().world.is_some(),
        "the server runs the world"
    );
    let gate = node(&server, "Garden gate");
    let door = node(&server, "Front door");
    let mut anna = join(&mut server);
    let mut ben = join(&mut server);
    let (anna_id, ben_id) = (server.id(&anna), server.id(&ben));

    // Anna opens the gate: the server checks she stands at it, and both
    // players get the new state.
    stand(&mut anna, [-10.0, 0.0, 0.0]);
    server.settle(&mut anna);
    use_object(&anna, gate);
    server.until(
        &mut [&mut anna, &mut ben],
        "the gate to open for both",
        |_, p| {
            p.iter()
                .all(|p| snapshot(p).is_some_and(|s| s.hidden.contains(&gate)))
        },
    );
    server.idle(&mut [&mut anna, &mut ben], 3);
    assert!(server
        .output()
        .contains(&format!("WORLDTEST used Garden gate by {anna_id}")));

    // A use from far away is refused: Ben is not at the front door.
    use_object(&ben, door);
    server.idle(&mut [&mut anna, &mut ben], 10);
    assert!(personal(&ben).is_empty(), "no teleport for a refused use");
    assert!(server.output().contains(&format!(
        "refused player {ben_id} using object {door} (not standing in it)"
    )));
    // Clients cannot pose as the server.
    ben.client
        .trigger(None, wire::STATE_EVENT, r#"[{"r":99,"s":[],"x":[]}]"#)
        .unwrap();
    server.idle(&mut [&mut anna, &mut ben], 5);
    assert!(snapshot(&anna).unwrap().hidden.contains(&gate));

    // Two uses in a row: the second is too fast.
    thread::sleep(world::USE_INTERVAL);
    use_object(&anna, gate);
    use_object(&anna, gate);
    server.idle(&mut [&mut anna, &mut ben], 10);
    assert!(server.output().contains("(too fast)"));
    // The one accepted use closed the gate again.
    server.until(&mut [&mut anna, &mut ben], "the gate to close", |_, p| {
        p.iter()
            .all(|p| snapshot(p).is_some_and(|s| !s.hidden.contains(&gate)))
    });

    // At the wheel, Anna cannot use the gate: it works on foot only.
    thread::sleep(world::USE_INTERVAL);
    let at = sa_scene::content_pack::ANCHOR;
    anna.place([at[0] - 10.0, at[1], 0.0], true);
    server.settle(&mut anna);
    use_object(&anna, gate);
    server.idle(&mut [&mut anna, &mut ben], 10);
    assert!(server
        .output()
        .contains("(on foot only, and the player is driving)"));
    assert!(!snapshot(&anna).unwrap().hidden.contains(&gate));
    stand(&mut anna, [-10.0, 0.0, 0.0]);
    server.settle(&mut anna);

    // A zone: Ben walks into the night zone. He gets the message, the time
    // changes for everyone, and scripts hear it.
    stand(&mut ben, [10.0, -10.0, 0.0]);
    server.until(&mut [&mut anna, &mut ben], "night for both", |_, p| {
        p.iter()
            .all(|p| snapshot(p).is_some_and(|s| s.hour == Some(22)))
    });
    server.idle(&mut [&mut anna, &mut ben], 3);
    assert_eq!(messages(&ben), vec!["Night falls"]);
    assert!(
        messages(&anna).is_empty(),
        "messages are only for the player"
    );
    assert!(server
        .output()
        .contains(&format!("WORLDTEST enter Night zone by {ben_id}")));
    stand(&mut ben, [0.0, -6.0, 0.0]);
    server.until(&mut [&mut anna, &mut ben], "noon again", |_, p| {
        p.iter()
            .all(|p| snapshot(p).is_some_and(|s| s.hour == Some(12)))
    });

    // The front door teleports Anna, and only her.
    stand(&mut anna, [0.0, 4.9, 0.0]);
    server.settle(&mut anna);
    // Her last use (the gate) may be too recent on a fast machine.
    thread::sleep(world::USE_INTERVAL);
    use_object(&anna, door);
    server.until(&mut [&mut anna, &mut ben], "the teleport", |_, p| {
        personal(p[0])
            .iter()
            .any(|e| matches!(e, wire::Personal::Teleport { position, .. } if position[0] == 60.0))
    });
    assert!(!personal(&ben)
        .iter()
        .any(|e| matches!(e, wire::Personal::Teleport { .. })));
}

#[test]
fn late_joiners_instances_scripts_and_departures() {
    let mut server = server();
    let gate = node(&server, "Garden gate");
    let mut anna = join(&mut server);
    stand(&mut anna, [-10.0, 0.0, 0.0]);
    server.settle(&mut anna);
    use_object(&anna, gate);
    server.until(&mut [&mut anna], "the gate to open", |_, p| {
        snapshot(p[0]).is_some_and(|s| s.hidden.contains(&gate))
    });

    // Carl joins later and gets the gate as it is now.
    let mut carl = join(&mut server);
    assert!(snapshot(&carl).unwrap().hidden.contains(&gate));

    // Carl goes to instance 5: a world of its own, with the gate closed.
    let carl_id = server.id(&carl);
    carl.client
        .trigger(None, "__cfx_internal:commandFallback", "[\"bucket 5\"]")
        .unwrap();
    server.until(
        &mut [&mut anna, &mut carl],
        "carl's own instance",
        |s, p| {
            s.session.routing_bucket(carl_id) == 5
                && snapshot(p[1]).is_some_and(|s| !s.hidden.contains(&gate))
        },
    );
    assert!(snapshot(&anna).unwrap().hidden.contains(&gate));

    // Scripts change the main world through natives; players see it.
    server.run("closegate");
    server.run("storm");
    server.until(
        &mut [&mut anna, &mut carl],
        "the script's changes",
        |_, p| {
            snapshot(p[0]).is_some_and(|s| {
                !s.hidden.contains(&gate)
                    && s.hour == Some(3)
                    && s.weather.as_deref() == Some("rain")
            })
        },
    );
    let output = server.output();
    assert!(output.contains("WORLDTEST closed true"), "{output}");
    assert!(output.contains("WORLDTEST gate visible true"), "{output}");
    assert_eq!(
        snapshot(&carl).unwrap().weather,
        Some("clear".into()),
        "instance 5 keeps its own weather"
    );

    // The console puts the main instance back as the world starts; Carl's
    // instance is left alone.
    let carl_revision = snapshot(&carl).unwrap().revision;
    server.run("world_reset 0");
    server.until(&mut [&mut anna, &mut carl], "the reset", |_, p| {
        snapshot(p[0]).is_some_and(|s| s.hour != Some(3) && s.weather.as_deref() != Some("rain"))
    });
    assert!(server.output().contains("World instance 0 reset"));
    assert_eq!(snapshot(&carl).unwrap().revision, carl_revision);
    server.run("world_reset 77");
    assert!(server
        .output()
        .contains("World instance 77 has no state yet"));
    server.run("world_reset five");
    assert!(server.output().contains("Usage: world_reset [instance]"));

    // Departed players are forgotten; the world stays as it is.
    server.leave(carl);
    server.idle(&mut [&mut anna], 3);
    assert_eq!(
        server
            .shared
            .borrow()
            .world
            .as_ref()
            .unwrap()
            .player_count(),
        1
    );
}

#[test]
fn teleports_wait_for_the_player_to_arrive_and_do_not_bounce() {
    let mut server = server();
    let mut anna = join(&mut server);
    let teleports = |p: &Player| {
        personal(p)
            .iter()
            .filter(|e| matches!(e, wire::Personal::Teleport { .. }))
            .count()
    };
    // The way-out pad inside the house leads to the spot outside.
    stand(&mut anna, [60.0, -8.0, 0.0]);
    server.until(&mut [&mut anna], "the pad's teleport", |_, p| {
        teleports(p[0]) == 1
    });
    // The game is slow to report the new place: no second teleport meanwhile.
    server.idle(&mut [&mut anna], 20);
    assert_eq!(teleports(&anna), 1);
    // It arrives outside: taken quietly, and nothing sends it back.
    stand(&mut anna, [0.0, 2.0, 0.0]);
    server.settle(&mut anna);
    server.idle(&mut [&mut anna], 20);
    assert_eq!(teleports(&anna), 1, "no bounce after arriving");
}

#[test]
fn servers_without_the_pack_run_no_world_gameplay() {
    let server = Sarebox::boot_with(
        "freeroam",
        &["ensure chat"],
        &["sets content_pack no-such-pack"],
        |_| {},
    );
    assert!(server.shared.borrow().world.is_none());
}
