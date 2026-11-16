# Make worlds and scripts with an AI assistant

SARE comes with **sa-mcp**, a creator bridge for AI assistants. It is a
[Model Context Protocol](https://modelcontextprotocol.io) (MCP) server: an
app such as Claude Desktop, Claude Code, Cursor or VS Code starts it, and the
assistant can then build Build Mode worlds and write Lua resources in your
SARE folders while you chat.

You stay in charge: the bridge only reads and writes files inside your
`content/` folder and your server's `resources/` folder. It never runs Lua,
never starts the game and never goes on the internet. Everything it makes is
a normal SARE file that you can open in Build Mode, edit by hand and share.

## What the assistant can do

| Tool | What it does |
| --- | --- |
| `sare_guide` | Reads SARE's guides, and the exact list of Lua functions this SARE has (topic `lua-api`), so it does not invent FiveM natives SARE does not have. |
| `list_worlds`, `create_world`, `get_world` | Lists, creates and reads Build Mode worlds (content packs). |
| `list_assets` | Lists the models that can be placed: built-in shapes, the world's own `.glb` files and models from other packs. |
| `place_objects`, `update_objects`, `delete_objects`, `set_spawn` | Edits a world. Each call is checked and saved in one go; a call with a mistake changes nothing. |
| `list_resources`, `read_resource_file` | Lists server resources and reads their files, for example `sare_rp_core` to learn its exports. |
| `create_resource` | Makes a new script resource from SARE's template (`fxmanifest.lua`, `server.lua`, `client.lua`) and can add `ensure <name>` to `server.cfg`. |
| `write_resource_file`, `check_lua` | Writes text files into a resource. Lua is syntax-checked first, so a broken script is refused with the line number. |

## 1. Get sa-mcp

Build it from the source (it is one small program):

```bash
cd native
cargo build --release -p sa-mcp
```

The program is `native/target/release/sa-mcp` (`sa-mcp.exe` on Windows).
Later releases will include it next to `sa-runtime` in the SARE folder.

## 2. Add it to your AI app

sa-mcp needs to know where your worlds and resources are:

- `--content-dir`: your SARE `content` folder (Build Mode worlds).
- `--resources-dir`: your server's `server-data/resources` folder (scripts).

Both are optional. Without them sa-mcp looks for `content/` and
`server-data/resources/` next to itself and in the folder it is started from.

### Claude Desktop

Open *Settings → Developer → Edit Config* and add SARE to
`claude_desktop_config.json` (use your own paths; on Windows, double the
backslashes):

```json
{
  "mcpServers": {
    "sare": {
      "command": "C:\\Users\\you\\Desktop\\SARE\\sa-mcp.exe",
      "args": [
        "--content-dir", "C:\\Users\\you\\Desktop\\SARE\\content",
        "--resources-dir", "C:\\Users\\you\\Desktop\\SARE Server\\server-data\\resources"
      ]
    }
  }
}
```

Restart Claude Desktop. "sare" shows up under the tools button in a new chat.

### Claude Code

```bash
claude mcp add sare -- /path/to/sa-mcp --content-dir /path/to/SARE/content --resources-dir /path/to/server-data/resources
```

### Other apps

Any app that supports MCP servers over stdio works the same way: the command
is the path to `sa-mcp`, the arguments are the two folders above.

## 3. Ask for something

Some prompts to start from:

- "Create a SARE world called Harbour. Put a road along the water, five
  houses in a row next to it, some trees and a few crates on the pier, and the
  spawn at the start of the road."
- "In my world `harbour`, group all the trees into a group called Park and
  move it 10 metres north."
- "Make a SARE resource called `fishing_job`: players type /fish at the pier,
  wait 5 seconds with a progress bar and get a fish. Use sare_rp_core exports
  like the other jobs do."
- "Read `sare_rp_bank` and explain how it adds money, then make a resource
  that pays players $50 every 10 minutes."

Then open the world in Build Mode (launcher → Edit, or
`sa-runtime --build harbour`) or restart your server to see it.

## Good to know

- **Close the world in Build Mode first**, or reopen it after the assistant
  is done (File → Open world). Build Mode does not see changes made while it
  has the world open, and saving there overwrites them.
- **Coordinates** are metres: x = east, y = north, z = up. Built-in shapes
  stand on their origin, so z = 0 puts them on a flat ground plate.
- The assistant sees object names and positions, not pictures. Look at the
  result in Build Mode and tell it what to fix ("the houses face the wrong
  way, turn them 180 degrees").
- Check scripts before you put them on a public server, like any script
  from someone else.
- Use original content only. Do not ask the assistant to copy game files or
  code from decompiled games; see the clean-room rules in `AGENTS.md`.

## For contributors

sa-mcp lives in `native/crates/mcp`. World tools use `sa-editor` (the same
code as Build Mode), script tools use SARE's CfxLua translator for the
syntax check, and the `lua-api` list is read from the engine source at build
time. Adding a tool is one entry in a `tools()` list with a JSON schema;
tests in the same file run it against a temporary folder. Ideas that would
help: an "add this job to my RP server" template, map importers, and a live
link to a running Build Mode so changes show up at once.
