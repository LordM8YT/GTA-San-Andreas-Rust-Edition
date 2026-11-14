//! SARE RP Town end to end: the `sare-rp` template's resources with real
//! client connections and the client scripts in the game's sandbox.
use super::sarebox_tests::{Player, Sarebox};
use super::*;
use serde_json::json;

// Places from resources/[sare-rp]/sare_rp_core/shared/locations.lua.
const SPAWN: [f32; 3] = [2445.0, -1712.0, 0.2];
const BANK: [f32; 3] = [2534.0, -1700.0, 0.2];
const SHOP: [f32; 3] = [2577.5, -1708.0, 0.2];
const POLICE_DESK: [f32; 3] = [2545.0, -1603.0, 0.2];
const JAIL_CELL: [f32; 3] = [2565.0, -1578.5, 0.2];
const JAIL_RELEASE: [f32; 3] = [2545.0, -1616.0, 0.2];
const DEALER: [f32; 3] = [2632.0, -1610.0, 0.2];
const GARAGE: [f32; 3] = [2522.0, -1794.0, 0.2];
const DELIVERY: [f32; 3] = [2642.0, -1648.0, 0.2];
const CLOTHES: [f32; 3] = [2442.5, -1791.5, 0.2];
const POLICE_GARAGE: [f32; 3] = [2530.0, -1640.0, 0.2];

fn rp_server() -> Sarebox {
    Sarebox::boot_with(
        "sare-rp",
        &[
            "ensure mapmanager",
            "ensure chat",
            "ensure spawnmanager",
            "ensure sarebox_core",
            "ensure sarebox_admin",
            "ensure [sare-rp]",
        ],
        &[],
        |_| {},
    )
}

/// The SARE Box HUD block ("$500  |  Bank $5000\nJob - Grade").
fn hud(player: &Player) -> String {
    player
        .text()
        .split("\n\n")
        .find(|block| block.contains("  |  Bank $"))
        .unwrap_or("")
        .to_string()
}
fn notified(player: &Player, text: &str) -> bool {
    player
        .host
        .ui()
        .notifications
        .iter()
        .any(|n| n.description.contains(text))
}
fn heard(player: &Player, text: &str) -> bool {
    player.chat.iter().any(|c| c.contains(text))
}
fn go(server: &mut Sarebox, player: &mut Player, at: [f32; 3]) {
    player.place(at, false);
    server.settle(player);
}
/// Press the interaction key and wait for the menu with `title`.
fn interact(server: &mut Sarebox, player: &mut Player, title: &str) {
    player.host.close_context();
    player.command("/rp_interact");
    server.until(&mut [player], title, |_, p| {
        p[0].host
            .ui()
            .context
            .as_ref()
            .is_some_and(|c| c.title == title)
    });
}
/// Pick the first menu option whose title starts with `title`.
fn pick(server: &mut Sarebox, player: &mut Player, title: &str) {
    let index = {
        let ui = player.host.ui();
        let menu = ui.context.as_ref().expect("an open menu");
        menu.options
            .iter()
            .position(|o| o.title.starts_with(title))
            .unwrap_or_else(|| panic!("no option {title}: {menu:?}"))
    };
    player.host.select_context(index);
    server.step(&mut [player]);
}
/// Wait for a dialog headed `heading` and confirm it with `values`.
fn answer(server: &mut Sarebox, player: &mut Player, heading: &str, values: Vec<Json>) {
    server.until(&mut [player], heading, |_, p| {
        p[0].host
            .ui()
            .dialog
            .as_ref()
            .is_some_and(|d| d.heading.starts_with(heading))
    });
    player.host.submit_dialog(Some(values));
    server.step(&mut [player]);
}
fn make_character(server: &mut Sarebox, player: &mut Player) {
    answer(
        server,
        player,
        "Create your character",
        vec![
            json!("anna"),
            json!("berg"),
            json!("1990-02-03"),
            json!("f"),
        ],
    );
    server.until(&mut [player], "the identity", |_, p| {
        notified(p[0], "You are Anna Berg.")
    });
    // Then the look: the creator opens on its own after the ID.
    server.until(&mut [player], "the character creator", |_, p| {
        p[0].host
            .ui()
            .context
            .as_ref()
            .is_some_and(|c| c.title == "Create your look")
    });
    pick(server, player, "Done");
    server.until(&mut [player], "the saved look", |_, p| {
        notified(p[0], "Welcome to town.")
    });
}

#[test]
fn rp_town_identity_chat_bank_shop_inventory_and_saves() {
    let mut server = rp_server();
    for resource in [
        "sare_rp_core",
        "sare_rp_identity",
        "sare_rp_chat",
        "sare_rp_bank",
        "sare_rp_inventory",
        "sare_rp_vehicles",
        "sare_rp_jobs",
        "sare_rp_admin",
        "sare-rp-map",
    ] {
        assert!(
            server.shared.borrow().resources[resource].started,
            "{resource} did not start: {}",
            server.output()
        );
    }
    let mut anna = server.join();

    // Identity: bad values are refused and the form comes back.
    answer(
        &mut server,
        &mut anna,
        "Create your character",
        vec![json!("A"), json!("b"), json!("yesterday"), json!("f")],
    );
    server.until(&mut [&mut anna], "the refusal", |_, p| {
        notified(p[0], "Use 2-16 letters")
    });
    make_character(&mut server, &mut anna);

    // Chat: /me reaches people close by, not people across town; /ooc is global.
    go(&mut server, &mut anna, SPAWN);
    let mut bob = server.join();
    bob.place([SPAWN[0] + 5.0, SPAWN[1], SPAWN[2]], false);
    server.settle(&mut bob);
    anna.command("/me waves");
    server.until(&mut [&mut anna, &mut bob], "/me nearby", |_, p| {
        heard(p[1], "* Anna Berg") && heard(p[1], "waves")
    });
    bob.place([SPAWN[0] + 120.0, SPAWN[1], SPAWN[2]], false);
    server.settle(&mut bob);
    thread::sleep(Duration::from_millis(750)); // chat rate limit
    anna.command("/me nods");
    server.until(&mut [&mut anna, &mut bob], "/me for anna", |_, p| {
        heard(p[0], "nods")
    });
    server.idle(&mut [&mut anna, &mut bob], 30);
    assert!(!heard(&bob, "nods"), "{:?}", bob.chat);
    thread::sleep(Duration::from_millis(750));
    anna.command("/ooc hello everyone");
    server.until(&mut [&mut anna, &mut bob], "/ooc", |_, p| {
        heard(p[1], "[OOC]") && heard(p[1], "hello everyone")
    });
    server.leave(bob);

    // Nothing to use in the middle of the park.
    anna.command("/rp_interact");
    server.until(&mut [&mut anna], "nothing here", |_, p| {
        notified(p[0], "nothing to use here")
    });

    // Bank: deposit at the counter; invalid amounts change nothing.
    go(&mut server, &mut anna, BANK);
    server.until(&mut [&mut anna], "the hint near the counter", |_, p| {
        p[0].text().contains("[H] Bank")
    });
    interact(&mut server, &mut anna, "Bank");
    pick(&mut server, &mut anna, "Deposit");
    answer(&mut server, &mut anna, "Deposit", vec![json!(1.5)]);
    server.until(&mut [&mut anna], "the refusal", |_, p| {
        notified(p[0], "whole number")
    });
    interact(&mut server, &mut anna, "Bank");
    pick(&mut server, &mut anna, "Deposit");
    answer(&mut server, &mut anna, "Deposit", vec![json!(200)]);
    server.until(&mut [&mut anna], "the deposit", |_, p| {
        hud(p[0]).starts_with("$300  |  Bank $5200")
    });
    // A made-up event far from the bank does nothing.
    go(&mut server, &mut anna, SPAWN);
    anna.client
        .trigger(None, "sare_rp_bank:server:action", "[\"withdraw\"]")
        .unwrap();
    server.until(&mut [&mut anna], "the bank check", |_, p| {
        notified(p[0], "Go to the bank")
    });

    // Shop: buy three waters with cash, then drink one from the inventory.
    go(&mut server, &mut anna, SHOP);
    interact(&mut server, &mut anna, "24/7 shop");
    pick(&mut server, &mut anna, "Water");
    answer(
        &mut server,
        &mut anna,
        "Buy Water",
        vec![json!(3), json!("cash")],
    );
    server.until(&mut [&mut anna], "the purchase", |_, p| {
        notified(p[0], "Bought 3x Water for $6.") && hud(p[0]).starts_with("$294")
    });
    anna.command("/inventory");
    server.until(&mut [&mut anna], "the inventory", |_, p| {
        p[0].host
            .ui()
            .context
            .as_ref()
            .is_some_and(|c| c.title == "Inventory")
    });
    pick(&mut server, &mut anna, "Water x3");
    server.until(&mut [&mut anna], "the item menu", |_, p| {
        p[0].host
            .ui()
            .context
            .as_ref()
            .is_some_and(|c| c.title == "Water")
    });
    pick(&mut server, &mut anna, "Use Water");
    server.until(&mut [&mut anna], "drinking", |_, p| {
        heard(p[0], "drinks some water")
    });

    // Everything is saved with the character and comes back after a rejoin.
    server.leave(anna);
    let mut anna = server.join();
    server.idle(&mut [&mut anna], 120);
    assert!(anna.host.ui().dialog.is_none(), "no second identity form");
    assert!(
        hud(&anna).starts_with("$294  |  Bank $5200"),
        "{}",
        hud(&anna)
    );
    anna.command("/inventory");
    server.until(&mut [&mut anna], "the saved inventory", |_, p| {
        p[0].host
            .ui()
            .context
            .as_ref()
            .is_some_and(|c| c.options.iter().any(|o| o.title == "Water x2"))
    });
    let saved = server.kvp().to_string();
    assert!(saved.contains("Anna") && saved.contains("water"), "{saved}");
    let output = server.output();
    assert!(!output.contains("SCRIPT ERROR"), "{output}");
}

#[test]
fn rp_town_police_duty_jail_and_dealer() {
    let mut server = rp_server();
    let mut officer = server.join();
    make_character(&mut server, &mut officer);
    let id = server.id(&officer);
    // Police is hired by an admin; it starts off duty.
    server.run(&format!("setjob {id} police 1"));
    server.until(&mut [&mut officer], "the job", |_, p| {
        hud(p[0]).contains("LSPD - Officer (off duty)")
    });
    // Jail needs an on-duty officer.
    let mut suspect = server.join();
    let suspect_id = server.id(&suspect);
    officer.command(&format!("/jail {suspect_id} 1"));
    server.until(&mut [&mut officer], "the duty check", |_, p| {
        notified(p[0], "Only on-duty LSPD")
    });
    go(&mut server, &mut officer, POLICE_DESK);
    interact(&mut server, &mut officer, "LSPD");
    pick(&mut server, &mut officer, "Go on duty");
    server.until(&mut [&mut officer], "on duty", |_, p| {
        hud(p[0]).contains("LSPD - Officer") && !hud(p[0]).contains("off duty")
    });
    // 911 reaches on-duty police with the caller's name and message.
    suspect.command("/911 robbery at the bank");
    server.until(&mut [&mut officer, &mut suspect], "the 911 call", |_, p| {
        heard(p[0], "[911]")
            && heard(p[0], "robbery at the bank")
            && notified(p[1], "reached 1 unit")
    });

    // Jail: the suspect must be close; then they go to the cell and stay there.
    suspect.place([POLICE_DESK[0] + 40.0, POLICE_DESK[1], 0.2], false);
    server.settle(&mut suspect);
    officer.command(&format!("/jail {suspect_id} 1"));
    server.until(
        &mut [&mut officer, &mut suspect],
        "the distance check",
        |_, p| notified(p[0], "Stand next to them"),
    );
    suspect.place([POLICE_DESK[0] + 2.0, POLICE_DESK[1], 0.2], false);
    server.settle(&mut suspect);
    officer.command(&format!("/jail {suspect_id} 1"));
    let near =
        |t: &[f32; 3], at: [f32; 3]| (t[0] - at[0]).abs() < 0.5 && (t[1] - at[1]).abs() < 0.5;
    server.until(&mut [&mut officer, &mut suspect], "the cell", |_, p| {
        p[1].teleports.iter().any(|t| near(t, JAIL_CELL)) && notified(p[1], "You are in jail")
    });
    // The suspect's game did not move (a modified game ignoring the
    // teleport): the server puts them back.
    server.until(
        &mut [&mut officer, &mut suspect],
        "the escape check",
        |_, p| notified(p[1], "cannot leave the cell"),
    );
    suspect.place(JAIL_CELL, false);
    server.settle(&mut suspect);
    officer.command(&format!("/unjail {suspect_id}"));
    server.until(&mut [&mut officer, &mut suspect], "the release", |_, p| {
        p[1].teleports.iter().any(|t| near(t, JAIL_RELEASE)) && notified(p[1], "released you")
    });
    server.leave(suspect);

    // Dealer: a sedan costs $9000 from the bank, in the colour picked.
    go(&mut server, &mut officer, DEALER);
    interact(&mut server, &mut officer, "Premium Motors");
    pick(&mut server, &mut officer, "Sedan");
    answer(&mut server, &mut officer, "Sedan", vec![json!("4")]);
    server.until(&mut [&mut officer], "the price check", |_, p| {
        notified(p[0], "You need $9000")
    });
    server.run(&format!("givemoney {id} bank 4500"));
    interact(&mut server, &mut officer, "Premium Motors");
    pick(&mut server, &mut officer, "Sedan");
    answer(&mut server, &mut officer, "Sedan", vec![json!("4")]);
    server.until(&mut [&mut officer], "the purchase", |_, p| {
        notified(p[0], "You bought the ocean blue Sedan") && hud(p[0]).contains("Bank $500")
    });
    let vehicles = |server: &Sarebox| {
        let shared = server.shared.borrow();
        shared
            .entities
            .all(sa_net::entities::EntityKind::Vehicle)
            .into_iter()
            .filter_map(|h| shared.entities.get(h).map(|e| e.state.model.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        vehicles(&server),
        vec![sa_net::entities::ModelRef::Name("sedan".into())]
    );
    // The delivered car carries the plate: the garage takes it in...
    go(&mut server, &mut officer, GARAGE);
    interact(&mut server, &mut officer, "Public garage");
    pick(&mut server, &mut officer, "Sedan [");
    server.until(&mut [&mut officer], "stored", |_, p| {
        notified(p[0], "is back in the garage")
    });
    assert!(vehicles(&server).is_empty());
    // ...and brings it out again; one car per player.
    interact(&mut server, &mut officer, "Public garage");
    pick(&mut server, &mut officer, "Sedan [");
    server.until(&mut [&mut officer], "taken out", |_, p| {
        notified(p[0], "is parked outside")
    });
    assert_eq!(vehicles(&server).len(), 1);
    interact(&mut server, &mut officer, "Public garage");
    pick(&mut server, &mut officer, "Sedan [");
    server.until(&mut [&mut officer], "stored again", |s, _| {
        vehicles(s).is_empty()
    });
    assert!(vehicles(&server).is_empty());
    let saved = server.kvp().to_string();
    assert!(saved.contains("sedan"), "{saved}");
    let output = server.output();
    assert!(!output.contains("SCRIPT ERROR"), "{output}");
}

fn wait_menu(server: &mut Sarebox, player: &mut Player, title: &str) {
    server.until(&mut [player], title, |_, p| {
        p[0].host
            .ui()
            .context
            .as_ref()
            .is_some_and(|c| c.title == title)
    });
}
fn got(player: &Player, name: &str, check: impl Fn(&str) -> bool) -> bool {
    player
        .events
        .iter()
        .any(|(n, payload)| n == name && check(payload))
}
fn look_of(server: &Sarebox, id: u32) -> Option<sa_net::appearance::Appearance> {
    server.shared.borrow().appearances.get(&id).copied()
}
fn sent_look(player: &Player, id: u32, look: sa_net::appearance::Appearance) -> bool {
    got(player, sa_net::appearance::APPEARANCE_EVENT, |payload| {
        sa_net::appearance::parse_event(payload).contains(&(id, Some(look)))
    })
}
fn enter(player: &mut Player, handle: u32) {
    player
        .client
        .trigger(None, "__sare:enterVehicle", &json!([handle]).to_string())
        .unwrap();
}
fn exit(player: &mut Player, handle: u32) {
    player
        .client
        .trigger(None, "__sare:exitVehicle", &json!([handle]).to_string())
        .unwrap();
}
fn granted(player: &Player, handle: u32, model: &str) -> bool {
    got(player, "__sare:vehicleGranted", |payload| {
        serde_json::from_str::<Json>(payload)
            .is_ok_and(|v| v[0] == json!(handle) && v[1] == json!(model))
    })
}
fn refused(player: &Player, handle: u32, reason: &str) -> bool {
    got(player, "__sare:vehicleRefused", |payload| {
        payload.contains(&handle.to_string()) && payload.contains(reason)
    })
}

#[test]
fn rp_town_looks_persist_and_sync_and_server_vehicles_drive() {
    use sa_net::entities::{parse_entities, EntityKind, ENTITIES_EVENT, ENTITY_DELETE_EVENT};
    let mut server = rp_server();
    let mut anna = server.join();
    let anna_id = server.id(&anna);

    // The creator starts from the ID (female body) and changes live.
    answer(
        &mut server,
        &mut anna,
        "Create your character",
        vec![
            json!("anna"),
            json!("berg"),
            json!("1990-02-03"),
            json!("f"),
        ],
    );
    server.until(&mut [&mut anna], "the creator", |_, p| {
        p[0].host
            .ui()
            .context
            .as_ref()
            .is_some_and(|c| c.title == "Create your look")
    });
    let start = look_of(&server, anna_id).expect("a starting look");
    assert_eq!(start.body, 1);
    pick(&mut server, &mut anna, "Hair:");
    server.until(&mut [&mut anna], "the hair change", |s, _| {
        look_of(s, anna_id).is_some_and(|l| l.hair == (start.hair + 1) % 7)
    });
    server.until(&mut [&mut anna], "the reopened creator", |_, p| {
        p[0].host
            .ui()
            .context
            .as_ref()
            .is_some_and(|c| c.options.iter().any(|o| o.title == "Hair: Ponytail"))
    });
    pick(&mut server, &mut anna, "Top colour:");
    wait_menu(&mut server, &mut anna, "Create your look");
    pick(&mut server, &mut anna, "Done");
    server.until(&mut [&mut anna], "the saved look", |_, p| {
        notified(p[0], "Welcome to town.")
    });
    let chosen = look_of(&server, anna_id).unwrap();
    assert_eq!(chosen.hair, 4);
    assert_ne!(chosen.top_color, start.top_color);

    // Another player gets the look from the server when they join.
    let mut bob = server.join();
    server.until(&mut [&mut anna, &mut bob], "anna's look at bob", |_, p| {
        sent_look(p[1], anna_id, chosen)
    });

    // Saved with the character: a rejoin wears it, without the creator.
    server.leave(anna);
    server.until(&mut [&mut bob], "anna forgotten", |_, p| {
        got(p[0], sa_net::appearance::APPEARANCE_EVENT, |payload| {
            sa_net::appearance::parse_event(payload).contains(&(anna_id, None))
        })
    });
    let mut anna = server.join();
    let anna_id = server.id(&anna);
    server.until(&mut [&mut anna, &mut bob], "the saved look", |s, _| {
        look_of(s, anna_id) == Some(chosen)
    });
    server.idle(&mut [&mut anna, &mut bob], 60);
    assert!(
        anna.host
            .ui()
            .context
            .as_ref()
            .is_none_or(|c| c.title != "Create your look"),
        "no second creator"
    );

    // The clothes shop sells a new top; everyone sees it.
    go(&mut server, &mut anna, CLOTHES);
    interact(&mut server, &mut anna, "Clothes shop");
    pick(&mut server, &mut anna, "Top:");
    wait_menu(&mut server, &mut anna, "Clothes shop");
    pick(&mut server, &mut anna, "Buy this outfit");
    server.until(&mut [&mut anna], "the purchase", |_, p| {
        notified(p[0], "Looking sharp!") && hud(p[0]).starts_with("$380")
    });
    let dressed = look_of(&server, anna_id).unwrap();
    assert_eq!(dressed.top, (chosen.top + 1) % 6);
    server.until(&mut [&mut anna, &mut bob], "the new top at bob", |_, p| {
        sent_look(p[1], anna_id, dressed)
    });
    let saved = server.kvp().to_string();
    assert!(
        saved.contains("appearance") && saved.contains("topColor"),
        "{saved}"
    );

    // Walking out of a fitting puts the paid-for clothes back on.
    interact(&mut server, &mut anna, "Clothes shop");
    pick(&mut server, &mut anna, "Top:");
    server.until(&mut [&mut anna], "the preview", |s, _| {
        look_of(s, anna_id).is_some_and(|l| l.top != dressed.top)
    });
    go(&mut server, &mut anna, DEALER);
    server.until(&mut [&mut anna], "the fitting to end", |s, p| {
        notified(p[0], "You left the clothes shop.") && look_of(s, anna_id) == Some(dressed)
    });
    assert!(hud(&anna).starts_with("$380"), "{}", hud(&anna));

    // A car from the dealer, in white.
    go(&mut server, &mut anna, DEALER);
    interact(&mut server, &mut anna, "Premium Motors");
    pick(&mut server, &mut anna, "Compact");
    answer(&mut server, &mut anna, "Compact", vec![json!("2")]);
    server.until(&mut [&mut anna], "the purchase", |_, p| {
        notified(p[0], "You bought the arctic white Compact")
    });
    let car = server.shared.borrow().entities.all(EntityKind::Vehicle)[0];
    assert_eq!(
        server
            .shared
            .borrow()
            .entities
            .get(car)
            .unwrap()
            .state
            .color,
        Some([235, 235, 235])
    );

    // Someone else cannot take it, and nobody drives it from afar.
    go(&mut server, &mut bob, [DELIVERY[0] + 2.0, DELIVERY[1], 0.2]);
    enter(&mut bob, car);
    server.until(&mut [&mut anna, &mut bob], "bob refused", |_, p| {
        refused(p[1], car, "cannot use") && notified(p[1], "belongs to someone else")
    });
    go(&mut server, &mut anna, SPAWN);
    enter(&mut anna, car);
    server.until(&mut [&mut anna, &mut bob], "too far", |_, p| {
        refused(p[0], car, "Too far")
    });

    // Anna gets in: her game gets the car, everyone else loses the entity.
    go(
        &mut server,
        &mut anna,
        [DELIVERY[0] - 2.0, DELIVERY[1], 0.2],
    );
    enter(&mut anna, car);
    server.until(&mut [&mut anna, &mut bob], "the grant", |_, p| {
        granted(p[0], car, "compact")
            && got(p[1], ENTITY_DELETE_EVENT, |d| d.contains(&car.to_string()))
    });
    assert!(got(&anna, "__sare:vehicleGranted", |p| p
        .contains("[235,235,235]")));
    assert_eq!(
        server.shared.borrow().entities.get(car).unwrap().driver,
        Some(anna_id)
    );
    enter(&mut bob, car);
    server.until(&mut [&mut anna, &mut bob], "taken", |_, p| {
        refused(p[1], car, "Someone is driving it")
    });

    // She drives 40 m east; the server entity follows her car.
    let parked = [DELIVERY[0] + 40.0, DELIVERY[1], 0.2];
    anna.place(parked, true);
    server.settle(&mut anna);
    server.until(
        &mut [&mut anna, &mut bob],
        "the entity to follow",
        |s, _| {
            s.shared
                .borrow()
                .entities
                .get(car)
                .is_some_and(|e| (e.state.position[0] - parked[0]).abs() < 0.1)
        },
    );
    // Out again: everyone gets the car back where she left it.
    bob.events.clear();
    anna.place([parked[0] - 2.0, parked[1], 0.2], false);
    exit(&mut anna, car);
    server.until(&mut [&mut anna, &mut bob], "the car back at bob", |_, p| {
        got(p[1], ENTITIES_EVENT, |payload| {
            parse_entities(payload)
                .iter()
                .any(|e| e.handle == car && (e.position[0] - parked[0]).abs() < 0.1)
        })
    });
    assert_eq!(
        server.shared.borrow().entities.get(car).unwrap().driver,
        None
    );

    // Job vehicles: the police cruiser puts the officer at the wheel, and
    // only on-duty police may drive it.
    server.run(&format!("setjob {anna_id} police 0"));
    go(&mut server, &mut anna, POLICE_DESK);
    interact(&mut server, &mut anna, "LSPD");
    pick(&mut server, &mut anna, "Go on duty");
    server.until(&mut [&mut anna], "on duty", |_, p| {
        hud(p[0]).contains("LSPD") && !hud(p[0]).contains("off duty")
    });
    go(&mut server, &mut anna, POLICE_GARAGE);
    interact(&mut server, &mut anna, "LSPD vehicles");
    pick(&mut server, &mut anna, "Take / return");
    let cruiser = |server: &Sarebox| {
        let shared = server.shared.borrow();
        shared
            .entities
            .all(EntityKind::Vehicle)
            .into_iter()
            .find(|h| {
                shared.entities.get(*h).unwrap().state.model
                    == sa_net::entities::ModelRef::Name("police".into())
            })
    };
    server.until(&mut [&mut anna], "the cruiser", |s, _| cruiser(s).is_some());
    let police = cruiser(&server);
    let police = police.expect("a police cruiser");
    server.until(&mut [&mut anna], "the warp", |_, p| {
        granted(p[0], police, "police")
    });
    exit(&mut anna, police);
    server.until(&mut [&mut anna], "out of the cruiser", |s, _| {
        s.shared
            .borrow()
            .entities
            .get(police)
            .unwrap()
            .driver
            .is_none()
    });
    go(
        &mut server,
        &mut bob,
        [POLICE_GARAGE[0] + 2.0, POLICE_GARAGE[1], 0.2],
    );
    enter(&mut bob, police);
    server.until(&mut [&mut anna, &mut bob], "bob refused", |_, p| {
        notified(p[1], "Only on-duty staff")
    });
    // Leaving the game gives the car back.
    enter(&mut anna, police);
    server.until(&mut [&mut anna], "back in", |s, _| {
        s.shared
            .borrow()
            .entities
            .get(police)
            .is_some_and(|e| e.driver == Some(anna_id))
    });
    server.leave(anna);
    server.until(&mut [&mut bob], "released", |s, _| {
        s.shared.borrow().drivers.is_empty()
    });
    let output = server.output();
    assert!(!output.contains("SCRIPT ERROR"), "{output}");
}
