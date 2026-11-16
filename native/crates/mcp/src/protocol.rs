//! The Model Context Protocol, as much as a tools-only server needs:
//! `initialize`, `ping`, `tools/list` and `tools/call` over JSON-RPC 2.0.
//! A failing tool answers with `isError: true` and a message the assistant
//! can act on, not with a protocol error.
use crate::{guide, scripts, worlds, Server};
use anyhow::Result;
use serde_json::{json, Value};

/// Protocol versions this server understands, newest first.
const VERSIONS: [&str; 3] = ["2025-06-18", "2025-03-26", "2024-11-05"];

const INSTRUCTIONS: &str = "SARE is an open-source game engine and modding platform. \
This server edits the creator's own SARE files: Build Mode worlds (content packs) and \
Lua server resources (FiveM-style fxmanifest.lua). Call sare_guide with topic \"start\" \
first, and \"lua-api\" before writing Lua, so you only use functions SARE has.";

pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub schema: fn() -> Value,
    pub run: fn(&Server, &Value) -> Result<Value>,
}

fn tools() -> Vec<Tool> {
    let mut all = vec![guide::tool()];
    all.extend(worlds::tools());
    all.extend(scripts::tools());
    all
}

/// Answer one line of input; None for notifications.
pub fn handle_line(server: &Server, line: &str) -> Option<String> {
    let reply = match serde_json::from_str::<Value>(line) {
        Ok(message) => handle(server, &message)?,
        Err(error) => error_reply(Value::Null, -32700, &format!("parse error: {error}")),
    };
    Some(reply.to_string())
}

pub fn handle(server: &Server, message: &Value) -> Option<Value> {
    let Some(method) = message.get("method").and_then(Value::as_str) else {
        // A response to us (we send no requests) or junk.
        return message
            .get("id")
            .filter(|_| message.get("result").is_none() && message.get("error").is_none())
            .map(|id| error_reply(id.clone(), -32600, "invalid request"));
    };
    // Notifications (no id) never get an answer.
    let id = message.get("id")?.clone();
    let params = message.get("params").cloned().unwrap_or(json!({}));
    let result = match method {
        "initialize" => {
            let asked = params["protocolVersion"].as_str().unwrap_or_default();
            let version = VERSIONS
                .iter()
                .find(|v| **v == asked)
                .unwrap_or(&VERSIONS[0]);
            json!({
                "protocolVersion": version,
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "sare", "title": "SARE creator bridge", "version": env!("CARGO_PKG_VERSION") },
                "instructions": INSTRUCTIONS,
            })
        }
        "ping" => json!({}),
        "tools/list" => json!({
            "tools": tools().iter().map(|t| json!({
                "name": t.name,
                "description": t.description,
                "inputSchema": (t.schema)(),
            })).collect::<Vec<_>>()
        }),
        "tools/call" => call(server, &params),
        _ => return Some(error_reply(id, -32601, &format!("unknown method {method}"))),
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn call(server: &Server, params: &Value) -> Value {
    let name = params["name"].as_str().unwrap_or_default();
    let arguments = match &params["arguments"] {
        Value::Null => json!({}),
        other => other.clone(),
    };
    let outcome = match tools().into_iter().find(|t| t.name == name) {
        Some(tool) => (tool.run)(server, &arguments),
        None => Err(anyhow::anyhow!("unknown tool {name:?}")),
    };
    match outcome {
        Ok(Value::String(text)) => json!({ "content": [{ "type": "text", "text": text }] }),
        Ok(value) => json!({
            "content": [{ "type": "text", "text": serde_json::to_string_pretty(&value).unwrap_or_default() }],
            "structuredContent": value,
        }),
        Err(error) => json!({
            "content": [{ "type": "text", "text": format!("{error:#}") }],
            "isError": true,
        }),
    }
}

fn error_reply(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_lists_tools_and_ignores_notifications() {
        let server = Server::default();
        let init = handle(
            &server,
            &json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}),
        )
        .unwrap();
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert!(handle(
            &server,
            &json!({"jsonrpc":"2.0","method":"notifications/initialized"})
        )
        .is_none());
        let list = handle(
            &server,
            &json!({"jsonrpc":"2.0","id":2,"method":"tools/list"}),
        )
        .unwrap();
        let tools = list["result"]["tools"].as_array().unwrap();
        let mut names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        for tool in tools {
            assert_eq!(tool["inputSchema"]["type"], "object", "{}", tool["name"]);
            assert!(!tool["description"].as_str().unwrap().is_empty());
        }
        let count = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), count, "tool names are unique");
        let unknown = handle(&server, &json!({"jsonrpc":"2.0","id":3,"method":"x"})).unwrap();
        assert_eq!(unknown["error"]["code"], -32601);
        let parse = handle_line(&server, "{nope").unwrap();
        assert!(parse.contains("-32700"));
    }

    #[test]
    fn tool_errors_are_results_the_assistant_can_read() {
        let server = Server::default();
        let reply = handle(
            &server,
            &json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"list_worlds","arguments":{}}}),
        )
        .unwrap();
        assert_eq!(reply["result"]["isError"], true);
        let text = reply["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("--content-dir"), "{text}");
    }
}
