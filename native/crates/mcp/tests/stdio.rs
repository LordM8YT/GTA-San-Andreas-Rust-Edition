//! The real binary over stdin/stdout, as an AI app runs it.
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
fn a_session_over_stdio_builds_a_world() {
    let content = std::env::temp_dir().join(format!("sare-mcp-stdio-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&content);
    std::fs::create_dir_all(&content).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_sa-mcp"))
        .arg("--content-dir")
        .arg(&content)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut ask = |message: Value| -> Option<Value> {
        writeln!(stdin, "{message}").unwrap();
        message.get("id")?;
        let mut line = String::new();
        stdout.read_line(&mut line).unwrap();
        Some(serde_json::from_str(&line).unwrap())
    };
    let init = ask(json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test","version":"1"}}})).unwrap();
    assert_eq!(init["result"]["serverInfo"]["name"], "sare");
    ask(json!({"jsonrpc":"2.0","method":"notifications/initialized"}));
    let call = |id: u64, name: &str, arguments: Value| json!({"jsonrpc":"2.0","id":id,"method":"tools/call","params":{"name":name,"arguments":arguments}});
    let made = ask(call(2, "create_world", json!({"name": "Stdio Town"}))).unwrap();
    assert_eq!(made["result"]["structuredContent"]["id"], "stdio-town");
    let placed = ask(call(
        3,
        "place_objects",
        json!({"world": "stdio-town", "objects": [{"asset": "builtin:house", "position": [5, 5, 0]}]}),
    ))
    .unwrap();
    assert!(placed["result"]["isError"].is_null(), "{placed}");
    let guide = ask(call(4, "sare_guide", json!({"topic": "start"}))).unwrap();
    assert!(guide["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("Build Mode"));
    drop(stdin);
    assert!(child.wait().unwrap().success());
    assert!(content.join("stdio-town/layout.json").is_file());
    std::fs::remove_dir_all(&content).unwrap();
}
