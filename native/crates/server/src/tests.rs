//! End-to-end: the shipped server-data resources with a real client socket.
use super::*;

const TESTER: &str = r#"
exports('double', function(x) return x * 2 end)
RegisterCommand('ping', function(src, args)
  TriggerClientEvent('test:pong', src, args[1], exports.tester:double(21), GetPlayerName(src))
end, false)
RegisterCommand('secret', function(src) SetConvar('secret_ran', tostring(src)) end, true)
AddEventHandler('chatMessage', function(src, name, message)
  if message == 'cancel me' then CancelEvent() end
end)
CreateThread(function()
  Wait(50)
  SetConvar('tester_thread', 'done')
end)
local v = vector3(1, 2, 3) + vector3(1, 1, 1)
SetConvar('tester_vector', ('%d %d %d %.1f'):format(v.x, v.y, v.z, #(vector3(3, 4, 0) - vector3(0, 0, 0))))
SetConvar('tester_json', json.encode({ a = 1 }))
"#;

fn temp_root() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let unique = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!("sare-server-{}-{unique}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    template::create(&root).unwrap();
    let tester = root.join("resources/[local]/tester");
    std::fs::create_dir_all(&tester).unwrap();
    std::fs::write(
        tester.join("fxmanifest.lua"),
        "fx_version 'cerulean'\ngame 'common'\nserver_script 'server.lua'\nclient_script 'client.lua'",
    )
    .unwrap();
    std::fs::write(tester.join("server.lua"), TESTER).unwrap();
    root
}

fn words(line: &str) -> Vec<String> {
    cfg::parse_line(line).remove(0)
}

#[test]
fn template_cfg_uses_only_known_commands() {
    let root = temp_root();
    let shared: Shared = Rc::new(RefCell::new(State::new(root.join("resources"))));
    shared.borrow_mut().capture = Some(String::new());
    let mut console = Console::new(shared.clone());
    console.execute_line(
        0,
        &format!("exec \"{}\"", root.join("server.cfg").display()),
    );
    let output = shared.borrow_mut().capture.take().unwrap();
    assert!(!output.contains("No such command"), "{output}");
    assert_eq!(
        console.boot_resources,
        [
            "mapmanager",
            "chat",
            "spawnmanager",
            "basic-gamemode",
            "sare-map-grove"
        ]
    );
    assert_eq!(console.endpoints[0].port(), 7777);
    assert_eq!(shared.borrow().convar("sv_hostname"), Some("SARE Freeroam"));
    assert!(shared.borrow().server_info.contains("sv_projectName"));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn default_resources_spawn_chat_commands_acl_and_exports() {
    let root = temp_root();
    let commands = [
        "endpoint_add_tcp 127.0.0.1:0",
        "sv_hostname \"Test Server\"",
        "sv_maxclients 4",
        "ensure basic-gamemode",
        "ensure chat",
        "ensure sare-map-grove",
        "ensure tester",
        "add_ace group.admin command allow",
    ]
    .iter()
    .map(|l| words(l))
    .collect();
    let Server {
        shared,
        mut console,
        session,
        publication: _,
    } = boot(commands, root.join("resources")).unwrap();
    assert_eq!(session.max_clients(), 4);
    for name in [
        "mapmanager",
        "spawnmanager",
        "basic-gamemode",
        "chat",
        "sare-map-grove",
        "tester",
    ] {
        assert!(
            shared.borrow().resources[name].started,
            "{name} not started"
        );
    }
    assert_eq!(shared.borrow().convar("gametype"), Some("Freeroam"));
    assert_eq!(shared.borrow().convar("mapname"), Some("sare-map-grove"));
    assert_eq!(shared.borrow().convar("tester_vector"), Some("2 3 4 5.0"));
    assert_eq!(shared.borrow().convar("tester_json"), Some(r#"{"a":1}"#));

    let client = sa_net::Session::join(session.address, "Guest").unwrap();
    let mut known = BTreeMap::new();
    let received: RefCell<Vec<sa_net::NetEvent>> = RefCell::new(Vec::new());
    let mut run = |console: &mut Console, until: &mut dyn FnMut(&[sa_net::NetEvent]) -> bool| {
        let start = Instant::now();
        loop {
            client.update(sa_net::Pose::default());
            pump(&shared, console, &session, &mut known).unwrap();
            received.borrow_mut().extend(client.events());
            if until(&received.borrow()) {
                return;
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "timed out; got {:?}",
                received.borrow()
            );
            thread::sleep(Duration::from_millis(5));
        }
    };
    let named = |events: &[sa_net::NetEvent], name: &str| -> Vec<Vec<Json>> {
        events
            .iter()
            .filter(|e| e.name == name)
            .map(|e| serde_json::from_str(&e.payload).unwrap())
            .collect()
    };

    // spawnmanager moves the joining player to a Grove Street spawn point.
    run(&mut console, &mut |e| {
        !named(e, "__sare:setCoords").is_empty()
    });
    let coords = &named(&received.borrow(), "__sare:setCoords")[0];
    let x = coords[0].as_f64().unwrap();
    assert!((2200.0..2520.0).contains(&x), "{coords:?}");
    run(&mut console, &mut |e| {
        !named(e, "chat:addSuggestions").is_empty()
    });
    assert!(named(&received.borrow(), "chat:addSuggestions")[0][0]
        .to_string()
        .contains("/respawn"));
    assert_eq!(shared.borrow().convar("tester_thread"), Some("done"));

    // Unrestricted script command from a player, through an export.
    client
        .trigger(None, "__cfx_internal:commandFallback", r#"["/ping hello"]"#)
        .unwrap();
    run(&mut console, &mut |e| !named(e, "test:pong").is_empty());
    assert_eq!(
        named(&received.borrow(), "test:pong")[0],
        vec![Json::from("hello"), Json::from(42), Json::from("Guest")]
    );

    // Restricted command needs the ACL.
    let id = shared.borrow().peers[0].id;
    client
        .trigger(None, "__cfx_internal:commandFallback", r#"["secret"]"#)
        .unwrap();
    client
        .trigger(
            None,
            "__cfx_internal:commandFallback",
            r#"["ping denied-check"]"#,
        )
        .unwrap();
    run(&mut console, &mut |e| named(e, "test:pong").len() == 2);
    assert_eq!(shared.borrow().convar("secret_ran"), None);
    console.execute_line(0, &format!("add_principal player.{id} group.admin"));
    client
        .trigger(None, "__cfx_internal:commandFallback", r#"["secret"]"#)
        .unwrap();
    let shared_check = shared.clone();
    run(&mut console, &mut |_| {
        shared_check.borrow().convar("secret_ran").is_some()
    });
    assert_eq!(
        shared.borrow().convar("secret_ran").map(str::to_string),
        Some(id.to_string())
    );

    // Chat: a cancelled message is not broadcast; a normal one is.
    client
        .trigger(
            None,
            "_chat:messageEntered",
            r#"["Guest",[255,255,255],"cancel me"]"#,
        )
        .unwrap();
    client
        .trigger(
            None,
            "_chat:messageEntered",
            r#"["Spoofed",[255,255,255],"hello all"]"#,
        )
        .unwrap();
    let chat = |e: &[sa_net::NetEvent]| {
        named(e, "chat:addMessage")
            .into_iter()
            .filter(|m| m[0]["args"][0] == "Guest")
            .collect::<Vec<_>>()
    };
    run(&mut console, &mut |e| !chat(e).is_empty());
    let messages = chat(&received.borrow());
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0][0]["args"][1], "hello all");

    // Events not registered with RegisterNetEvent are ignored from clients.
    client
        .trigger(None, "chatMessage", r#"[1,"x","y"]"#)
        .unwrap();

    // Live resource control.
    console.execute_line(0, "restart tester");
    assert!(shared.borrow().resources["tester"].started);
    console.execute_line(0, "stop tester");
    assert!(!shared.borrow().commands.contains_key("ping"));
    console.execute_line(0, "clientkick 999 nobody");
    console.execute_line(0, &format!("clientkick {id} Bye"));
    let start = Instant::now();
    while !client
        .update(sa_net::Pose::default())
        .is_some_and(|r| r.status.contains("Bye"))
    {
        assert!(start.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(5));
    }
    drop(session);
    drop(shared);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn category_ensure_starts_scripts_and_enabled_native_resources_only() {
    let root = temp_root();
    for (name, enabled) in [("native-on", true), ("native-off", false)] {
        let folder = root.join("resources/[local]").join(name);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("mod.json"),
            format!(r#"{{"schema_version":2,"enabled":{enabled},"name":"{name}"}}"#),
        )
        .unwrap();
    }
    let commands = ["endpoint_add_tcp 127.0.0.1:0", "ensure [local]"]
        .iter()
        .map(|l| words(l))
        .collect();
    let Server {
        shared,
        mut console,
        ..
    } = boot(commands, root.join("resources")).unwrap();
    let started = |name: &str| shared.borrow().resources[name].started;
    assert!(started("tester") && started("native-on") && !started("native-off"));
    assert!(!started("chat"));
    // Native assets are fixed after boot; script resources stay live.
    console.execute_line(0, "stop native-on");
    assert!(started("native-on"));
    console.execute_line(0, "ensure [system]");
    assert!(started("chat"));
    drop(console);
    drop(shared);
    let _ = std::fs::remove_dir_all(root);
}
