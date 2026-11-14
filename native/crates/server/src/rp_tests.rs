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

    // Dealer: SARE Kart costs $6500 from the bank; then the garage keeps it.
    go(&mut server, &mut officer, DEALER);
    interact(&mut server, &mut officer, "Premium Motors");
    pick(&mut server, &mut officer, "SARE Kart");
    server.until(&mut [&mut officer], "the price check", |_, p| {
        notified(p[0], "You need $6500")
    });
    server.run(&format!("givemoney {id} bank 2000"));
    interact(&mut server, &mut officer, "Premium Motors");
    pick(&mut server, &mut officer, "SARE Kart");
    server.until(&mut [&mut officer], "the purchase", |_, p| {
        notified(p[0], "Bought a SARE Kart") && hud(p[0]).contains("Bank $500")
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
        vec![sa_net::entities::ModelRef::Hash(i64::from(
            sa_lua::cfxlua::joaat("SARE Kart")
        ))]
    );
    // The delivered car carries the plate: the garage takes it in...
    go(&mut server, &mut officer, GARAGE);
    interact(&mut server, &mut officer, "Public garage");
    pick(&mut server, &mut officer, "SARE Kart [");
    server.until(&mut [&mut officer], "stored", |_, p| {
        notified(p[0], "is back in the garage")
    });
    assert!(vehicles(&server).is_empty());
    // ...and brings it out again; one car per player.
    interact(&mut server, &mut officer, "Public garage");
    pick(&mut server, &mut officer, "SARE Kart [");
    server.until(&mut [&mut officer], "taken out", |_, p| {
        notified(p[0], "is parked outside")
    });
    assert_eq!(vehicles(&server).len(), 1);
    interact(&mut server, &mut officer, "Public garage");
    pick(&mut server, &mut officer, "SARE Kart [");
    server.until(&mut [&mut officer], "stored again", |s, _| {
        vehicles(s).is_empty()
    });
    assert!(vehicles(&server).is_empty());
    let saved = server.kvp().to_string();
    assert!(saved.contains("SARE Kart"), "{saved}");
    let output = server.output();
    assert!(!output.contains("SCRIPT ERROR"), "{output}");
}
