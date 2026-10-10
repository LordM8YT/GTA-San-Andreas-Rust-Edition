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
-- ox_lib sets state bags with msgpack-packed values.
local packed = msgpack.pack({ a = 1, b = { 'x' } })
SetStateBagValue('global', 'packed', packed, #packed, true)
SetConvar('tester_bag', json.encode(GlobalState.packed) .. ' ' .. json.encode(msgpack.unpack(packed)))
SetConvar('tester_bool', tostring(GetConvarBool('missing_bool', true)))
-- CfxLua syntax and library, also in chunks compiled with load().
local n, none = 1, nil
n += 2
local loaded = load('local t = { a = 1 } t.a *= 5 return t?.a')()
SetConvar('tester_cfx', ('%d %d %s %d %s %s %s'):format(n, `adder`, tostring(none?.x), loaded,
  table.type({ 1, 2 }), table.type({ a = 1 }), (select(2, string.strsplit(':', 'a:b:c', 2)))))
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
    assert_eq!(
        shared.borrow().convar("tester_bag"),
        Some(r#"{"a":1,"b":["x"]} {"a":1,"b":["x"]}"#)
    );
    assert_eq!(shared.borrow().convar("tester_bool"), Some("true"));
    assert_eq!(
        shared.borrow().convar("tester_cfx"),
        Some("3 -1216765807 nil 5 array hash b:c")
    );

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

#[test]
fn framework_examples_use_includes_provide_function_refs_and_state_bags() {
    let root = temp_root();
    let commands = [
        "endpoint_add_tcp 127.0.0.1:0",
        "set sare_core_bannedNames \"Banned, Other\"",
        "ensure chat",
        "ensure sare_jobs",
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
    // sare_jobs depends on `sare-core`, which sare_core provides.
    for name in ["sare_lib", "sare_core", "sare_jobs"] {
        assert!(
            shared.borrow().resources[name].started,
            "{name} not started"
        );
    }
    assert_eq!(
        script::resolve(&shared, "sare-core").as_deref(),
        Some("sare_core")
    );

    let client = sa_net::Session::join(session.address, "Guest").unwrap();
    let mut known = BTreeMap::new();
    let mut received: Vec<sa_net::NetEvent> = Vec::new();
    let run = |console: &mut Console,
               known: &mut BTreeMap<u32, String>,
               received: &mut Vec<sa_net::NetEvent>,
               until: &dyn Fn(&[sa_net::NetEvent]) -> bool| {
        let start = Instant::now();
        loop {
            client.update(sa_net::Pose::default());
            pump(&shared, console, &session, known).unwrap();
            received.extend(client.events());
            if until(received) {
                return;
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "timed out; got {received:?}"
            );
            thread::sleep(Duration::from_millis(5));
        }
    };
    let messages = |events: &[sa_net::NetEvent]| -> Vec<String> {
        events
            .iter()
            .filter(|e| e.name == "chat:addMessage")
            .map(|e| {
                serde_json::from_str::<Vec<Json>>(&e.payload).unwrap()[0]["args"][1].to_string()
            })
            .collect()
    };
    let check = shared.clone();
    run(&mut console, &mut known, &mut received, &|_| {
        check
            .borrow()
            .bags
            .get("global")
            .and_then(|b| b.get("players"))
            == Some(&Json::from(1))
    });
    let id = shared.borrow().peers[0].id;
    let bag = format!("player:{id}");
    assert_eq!(shared.borrow().bags[&bag]["cash"], Json::from(500));

    // /job runs SetJob in sare_core through a function reference; the
    // state bag change handler in sare_jobs notifies the player.
    let send = |line: &str| {
        client
            .trigger(
                None,
                "__cfx_internal:commandFallback",
                &Json::Array(vec![line.into()]).to_string(),
            )
            .unwrap()
    };
    send("/job taxi");
    run(&mut console, &mut known, &mut received, &|e| {
        messages(e)
            .iter()
            .any(|m| m.contains("You now work as taxi"))
    });
    assert_eq!(shared.borrow().bags[&bag]["job"], Json::from("taxi"));

    // /work: AddMoney through the player object, then a callback that
    // sare_lib stores and calls back into sare_core.
    send("/work");
    run(&mut console, &mut known, &mut received, &|e| {
        messages(e).iter().any(|m| m.contains("Cash: $620"))
    });
    assert_eq!(shared.borrow().bags[&bag]["cash"], Json::from(620));

    // Per-call callback references are freed; long-lived ones stay bounded.
    for _ in 0..3 {
        pump(&shared, &mut console, &session, &mut known).unwrap();
    }
    let held = shared.borrow().refs.len();
    send("/work");
    send("/work");
    run(&mut console, &mut known, &mut received, &|e| {
        messages(e).iter().any(|m| m.contains("Cash: $860"))
    });
    for _ in 0..3 {
        // Proxies are collected by Lua's GC; force it so the count is stable.
        for resource in ["sare_lib", "sare_core", "sare_jobs"] {
            let lua = shared.borrow().resources[resource].lua.clone().unwrap();
            lua.gc_collect().unwrap();
            lua.gc_collect().unwrap();
        }
        pump(&shared, &mut console, &session, &mut known).unwrap();
    }
    assert!(
        shared.borrow().refs.len() <= held,
        "{} > {held}",
        shared.borrow().refs.len()
    );

    // playerConnecting deferrals reject a banned name.
    let banned = sa_net::Session::join(session.address, "Banned").unwrap();
    let start = Instant::now();
    loop {
        pump(&shared, &mut console, &session, &mut known).unwrap();
        if banned
            .update(sa_net::Pose::default())
            .is_some_and(|r| r.status.contains("not allowed"))
        {
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(5));
        thread::sleep(Duration::from_millis(5));
    }

    // Stopping the core makes its references unusable without crashing users.
    console.execute_line(0, "stop sare_core");
    assert!(shared
        .borrow()
        .refs
        .keys()
        .all(|k| !k.starts_with("sare_core:")));
    shared.borrow_mut().capture = Some(String::new());
    send("/work");
    let check = shared.clone();
    run(&mut console, &mut known, &mut received, &|_| {
        check
            .borrow()
            .capture
            .as_ref()
            .is_some_and(|c| c.contains("no longer exists"))
    });
    drop(session);
    drop(shared);
    let _ = std::fs::remove_dir_all(root);
}
