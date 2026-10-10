//! Lua scripting shared by the server and the game client: CfxLua translation,
//! the common FiveM-style prelude, client script bundles, the sandboxed client
//! script host and the UI model it drives.
pub mod bundle;
pub mod cfxlua;
pub mod client;
pub mod json;
pub mod ui;

/// Scheduler, events, exports, function references, vectors, json and the
/// CfxLua library. Each side appends its own prelude to this chunk.
pub const COMMON_PRELUDE: &str = include_str!("common.lua");
