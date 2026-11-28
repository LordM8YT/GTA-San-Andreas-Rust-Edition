# World scripts

World scripts are Lua or JavaScript that lives inside a Build Mode world. Put one on an
object, or one for the whole world, and it runs whenever the world is
played: in Build Mode's Play (F5), from the launcher, and on servers that
host the world. You need no server folder, resource or manifest.

```lua
-- On a door with an Interaction behaviour: E opens it for 5 seconds.
local door = script.object

door:on('use', function(player)
  door:hide()
  player:message('The door is open for 5 seconds')
  wait(5)
  door:show()
end)
```

World scripts sit beside the [gameplay behaviours](build-mode.md#gameplay)
and do not replace them. A behaviour gives an object its zone (a trigger
zone, an interaction for E, a checkpoint). A script decides what happens
there, with as much logic as you like.

## JavaScript

Pick an example marked *(JavaScript)* under *+ Add script* to write a
script in JavaScript instead. It has the same API, written the JavaScript
way: methods with a dot, `null` for nothing, and `await wait(seconds)` to
pause (a script's top level and its handlers may use `await`; give
handlers `async`).

```js
// The door above, in JavaScript.
const door = script.object;

door.on('use', async (player) => {
  door.hide();
  player.message('The door is open for 5 seconds');
  await wait(5);
  door.show();
});
```

| Lua | JavaScript |
|---|---|
| `door:hide()`, `door:moveTo(v, 2)` | `door.hide()`, `door.moveTo(v, 2)` |
| `wait(2)` | `await wait(2)` |
| `a + b`, `v * 2` on vectors | `a.add(b)`, `v.mul(2)`, also `sub`, `div`, `neg`, `equals` |
| `door.color = nil` | `door.color = null` |
| `for _, o in ipairs(world.tagged('Lamp'))` | `for (const o of world.tagged('Lamp'))` |
| `tostring(v)` | `String(v)` or `` `${v}` `` |

Both languages run on the same natives, so a JavaScript script and a Lua
script in one world see and change the same objects, and the same limits
apply (time per frame, memory). JavaScript runs on QuickJS, a small engine
built into SARE; there are no modules or `import` yet. A script's language
is stored as `"language": "javascript"` in `layout.json`; scripts without it
are Lua.

## In Build Mode

- **Script** on the Home ribbon (Insert) adds a script to the selected
  object, or to the world when nothing is selected, and opens it.
- An object's **Properties** have a **Scripts** section. Use **+ Add
  script** to start from an example, the box to switch a script on or off,
  and 🗑 to delete it. The same section has **Scripts can change it** (see
  below).
- The **Explorer** lists an object's scripts under it (📜). Its
  **Scripts** row lists the world's own scripts and every object that has
  some.
- The **script editor** colours Lua and checks it as you type: a syntax
  error is shown above the text, and its line is underlined. It also has
  **+ Example** to add an example to the end of the script, and **? Help**
  for a short list of the API. What you type becomes one undo step after a
  pause, when the window closes, and before saving or playing.
- While you play, `print` and errors show in **Output** with the script's
  name and line, for example `Script error in Door/Opener: Door/Opener:4:
  attempt to index a nil value`.

The examples are Door that opens on E, Welcome message, Blinking object,
Zone messages, Lift, Sliding door, Spinner, Traffic light, Race results, Day and night
switch and Score counter.

Scripts are saved in the world's `layout.json`, as `scripts` on an object
or in `settings.scripts`, each with a `name`, its `source` and `enabled`.
A world holds at most 64 scripts of up to 64 KiB each.

## The API

Positions are the world's metres, as the Properties panel shows them: x
east, y north, z up. Headings are degrees, and 0 faces north.

| | |
|---|---|
| `script.name`, `script.object` | This script's name, and the object it is on (nil for world scripts). |
| `world.find(name)` | The first object with that name, or nil. |
| `world.objects()`, `world.players()` | Every object; every player in the world. |
| `world.message(text, seconds)` | A message for everyone in the world (4 seconds by default). |
| `world.hour()`, `world.setHour(0-23)` | The hour of day. A running clock jumps to the new hour and runs on. |
| `world.weather()`, `world.setWeather(name)` | `clear`, `cloudy`, `rain` or `fog`. |
| `world.time()` | Seconds since the scripts started. |
| `world.on(event, [object or name,] fn)` | Listen to an event; returns a handle with `:disconnect()`. |
| `world.emit(name, ...)` | Send values to every script's `world.on(name, ...)` from the next frame on. |
| `object.id`, `.name` | |
| `object.visible` | Read it, or set it to show or hide the object. |
| `object.position`, `.heading` | Where the object is now. Set them to move it at once. |
| `object.color` | Set it to `rgb(red, green, blue)` (0 to 1, up to 2 brightens) to colour the object, or `nil` for its own colours. Works on models, not on terrain or roads. |
| `object:show()`, `:hide()`, `:toggle()` | |
| `object:moveTo(vector, seconds, [heading])` | Glide there, easing in and out. |
| `object:turnTo(heading, seconds)` | Turn, the short way round. A new move replaces the one on its way and keeps the half it does not change: `moveTo` keeps where it was turning to, `turnTo` where it was going. |
| `object:on(event, fn)` | `world.on(event, object, fn)`. |
| `object:clone([vector], [heading])` | A copy of the object, where it is now or at the given place and heading. It is shown, in the object's colour, and scripts can show, hide, move, colour and destroy it like the object itself. A copy has no zones, scripts or light of its own and is not saved: copies go when the world starts again. A world can have at most 200 copies at once. Works on models, not on terrain or roads. |
| `object:get(name)`, `object:set(name, value)` | The object's attributes (Properties > Attributes): true/false, a number, a text or a vector, or nil when it has none. An object without one of that name gets its group's. `set` changes or adds one (nil removes it) while the world runs; the world's file is not changed. At most 64 per object, names up to 64 characters, texts up to 1024 bytes. |
| `object.tags`, `object:hasTag(tag)`, `object:addTag(tag)`, `object:removeTag(tag)` | The object's tags, with those of the groups it is in. Adding and removing lasts while the world runs. |
| `world.tagged(tag)` | Every object with that tag, in world order: `for _, coin in ipairs(world.tagged('Coin')) do ... end`. |
| `object:destroy()` | Remove a copy for good. Objects placed in the world cannot be destroyed; hide them instead. |
| `object.original`, `.exists` | The object a copy was made from (nil for placed objects), and whether the object is still in the world (false once destroyed). |
| `player.id`, `.name`, `.position`, `.driving` | `position` is nil until the player has spawned. |
| `player:message(text, seconds)` | A message for this player only. |
| `player:teleport(object or vector, [heading])` | Onto an object (facing its way), or to a spot. |
| `player:hud(name, text)`, `world.hud(name, text)` | A line of on-screen text for this player, or for everyone, until it changes. Set it again under the same name to change it; `nil` or `''` removes it. At most 12 lines for everyone and 12 per player, of up to 120 characters each; everyone's lines come first. A player's own lines go when the scripts start again (`world_reset`) or the player changes instance; for a player who left, it does nothing. |
| `vector(x, y, z)` | Add, subtract, multiply by a number, `:length()`, `:distance(v)`. |
| `rgb(r, g, b)` | A colour for `object.color`. |
| `wait(seconds)` | Pause this script or handler. Other scripts go on. |
| `spawn(fn)`, `delay(seconds, fn)`, `every(seconds, fn)` | Run something now, later, or again and again. `every` returns a handle with `:stop()`. |
| `print(...)` | Writes to Output, or to the server console. |

### Events

| Event | Handler gets | When |
|---|---|---|
| `enter`, `leave` | `player, object` | A player walks or drives into or out of an object's trigger zone. |
| `use` | `player, object` | A player presses E at an object's interaction. |
| `checkpoint` | `player, race, number, total` | A player reaches a checkpoint (the start is number 1). |
| `finish` | `player, race, seconds` | A player finishes a checkpoint race. |
| `join`, `quit` | `player` | A player comes into the world, or leaves it. |
| your own name | the values sent | Another script called `world.emit(name, ...)`. |

An interaction or trigger zone needs no actions of its own when a script
listens to it. The editor only warns about one that has neither actions nor
a script on it or naming it.

### Objects scripts can change

Scripts may show, hide, move, colour and copy:

- the object a script is on;
- objects whose name a script writes in quotes, such as
  `world.find("Lamp")`;
- objects ticked **Scripts can change it** in their Properties. Tick this
  for objects that a script finds in another way, for example by a name it
  builds from parts.

These objects are drawn and made solid one by one, like doors, so tick it
only where you need it. If a script tries to change any other object, the
error tells you which box to tick.

A moved object takes its collision along, and a player standing on it
rides with it (a lift). Its zones stay where the object was placed. Moves
are shared like doors: every player sees the same lift, also players who
join halfway.

## How scripts run

- Each script has its own Lua sandbox. It gets `table`, `string`, `math`,
  `utf8`, `coroutine` and `os.clock`, but no files, processes, `require` or
  `debug`, and `load` takes text only. Scripts talk to each other with
  `world.emit`.
- A script body runs once when the world starts, until its first `wait`.
  Handlers each run on their own, so one handler can `wait` without holding
  up the others.
- Limits: 16 MiB of memory per script, and 250 ms of work per script per
  frame across all its calls. A script that goes over is stopped until the
  world starts again, even if it catches the stop with `pcall`. Keep loops
  short, or `wait` in them.
- String patterns that could take too long on a long text are refused with
  an error (pattern matching runs in C, where the time limit cannot stop
  it). `find` with `plain` set to true is always allowed.
- At most 2,000 changes per frame across all scripts, messages up to 500
  characters, and `world.emit` up to 4 KiB per call and 200 calls per
  frame.

### Alone and on servers

- **Play (F5) and the launcher:** the game runs the scripts for you, the
  one player.
- **On a server** (`sets content_pack <id>`, see
  [server-hosting](server-hosting.md#build-mode-worlds)): the server runs
  the scripts once per world instance (routing bucket), with every player
  of that instance. What they change is shared like a door. Messages and
  teleports for one player go to that player. `print` and errors go to the
  server console, with `[instance N]` before them for instances other than
  0. A joined game never runs the scripts itself.
- An instance's scripts start when its first player arrives and stop 30
  seconds after its last player leaves. The shared state stays, and they
  start again when someone comes back. `world_reset` starts them again
  from the beginning.
- Copies scripts made stay in the shared state while the scripts are
  stopped. When they start again, the copies are in `world.objects()`
  (their `original` is set), so the scripts can change or destroy them.
  `world_reset` removes every copy.
- Server resources still get the `sare:*` events and world natives (see
  [server-hosting](server-hosting.md#build-mode-worlds)) beside the
  world's own scripts.

## Not done yet

- Moving zones: a moved object's trigger or interaction zone stays where
  it was placed.
- Tilting objects: scripts set the heading only.
- Scripts on the player's own game (UI, input); see
  [client scripts](client-scripts.md) for server resources.
- Spawning new objects, sounds, and changing sizes from scripts.
