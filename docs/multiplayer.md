# Player-hosted multiplayer prototype

Open **Multiplayer** from the main/pause menu, press **F5** in free roam, or
type **/mp**. Set your name, then choose **Host session** or **Join session**.
The limit is **20 total players: one host and 19 guests**.

There are two connection modes: **direct IP** and **player hosting via relay**.
The direct host normally listens on `0.0.0.0:7777`. Guests enter the host's actual IP,
for example `192.168.1.10:7777`; do not join `0.0.0.0`. On the same computer,
use `127.0.0.1:7777`. For an internet connection the host needs an accessible
public address, a firewall rule allowing the application, and TCP port 7777
forwarded to their computer. CGNAT can prevent direct hosting; a shared VPN
network is another option. There is no automatic NAT traversal, lobby service,
UPnP or automatic matchmaking in direct mode.
Relay mode provides a server browser and join codes; see the setup below.
Native server resources now download into an isolated, verified cache before joining.

## Player hosting via relay and server browser

The new mode makes outbound connections from both host and guests. The game
host listens only on loopback behind the tunnel, owns player membership and
continues running the game. The relay runs a session directory and forwards
bytes; it does not simulate the game or distribute assets. This is
relay-assisted player hosting, not direct NAT-traversing P2P. All gameplay
traffic in this mode goes through the relay.

1. Run `start-relay.cmd` on Windows. By default it listens on
   `127.0.0.1:7778`, suitable for testing on one PC. For a trusted LAN/VPN,
   run `start-relay.cmd 0.0.0.0:7778` and allow TCP 7778 on that machine.
   Linux can build/run `cargo run --release -p sa-net --bin sa-relay -- 0.0.0.0:7778`
   from `native`; Linux gameplay testing remains pending.
2. In Multiplayer, enable **Player hosting via relay / join code** and enter
   the relay's reachable IP and port. Every participant uses the same relay.
   `127.0.0.1` refers to the participant's own PC; use the relay machine's
   LAN/VPN IP for separate PCs. `SA_RELAY_ADDRESS` can set the initial address.
3. The host enables **Show my session in the server browser** for a public
   listing, or leaves it off for a private room. Choose **Host session**.
   Copy/share the displayed 12-character join code after publication succeeds.
4. Guests choose **Refresh server browser**, then **Join** beside a public
   room, or enter the private join code and choose **Join session**.
5. Enter free roam. Disconnecting the host withdraws the room and closes its
   tunnels. If the relay disappears, the session ends; there is no relay
   failover or host migration.

The game host needs no inbound port forwarding in this mode. The relay must
itself be reachable by everyone; running it on the host's PC behind the same
router does not remove that router requirement. No public relay is deployed
or configured by default. Internet hosting needs a reachable relay machine
or service; for this prototype, use a trusted LAN/VPN.

Private rooms are omitted from the browser. Their codes contain 48 random
bits and act as bearer invitations: anyone given the code can join. A separate
256-bit host credential protects tunnel attachment and is not exposed in the
browser. Credentials and gameplay traffic currently travel over **unencrypted
TCP**. This is not a hardened public service: TLS/authentication, IP rate limits,
operational monitoring and production relay deployment are still pending.
The relay is trusted and can see traffic. Steam lobby/relay integration is
not included; the project currently has no Steamworks App ID.

The service bounds rooms (32), worker connections (256), guest tunnels
(20 per room), control frame size (8 KiB), handshake time and tunnel buffers.
These limits are resource bounds, not evidence of production-scale capacity.

Enter free roam after connecting. The session page shows the connection status
and player list; gameplay shows the player count. Opening a menu pauses your
own movement while the network and other players continue. **Disconnect**
leaves the session. Returning to the main menu also disconnects. If the host
quits, guests return to an offline world and can reconnect through Multiplayer.
There is no host migration.

Each player needs their own original San Andreas installation and this runtime.
Original game archives are not distributed. Enabled native mods are shared as
an immutable inventory; see the resource preparation below.

## Native server resource preparation

Hosting exports only files referenced by enabled `mod.json` / `resource.json`
resources. Guests query this ordered inventory before gameplay. Keep
**Automatically download required server mods** enabled to fetch missing files;
with it disabled, joining still works when every required file is already cached.
This preference is saved with other settings.
The prepared inventory fingerprint is checked again during login, so a restarted
host with different resources cannot silently admit an outdated client.

The cache is `%LOCALAPPDATA%/SAFreeroam/server-cache` on Windows, or
`$XDG_CACHE_HOME/sa-freeroam/server-cache` / `$HOME/.cache/sa-freeroam/server-cache`
on Linux. `--cache-dir <directory>` overrides it for isolated tests. SHA-256
content blobs are reused across sessions and changed versions; completed packs
retain the resource order. Every reused file is checked again before loading.

Bounds: 16 resources, 64 files, 16 MiB per file, 128 MiB per inventory, 12 KiB
inventory metadata and 512 MiB total cache. Only native DFF/TXD/PNG/COL/IFP and
resource manifests are accepted. Traversal, Windows device paths, scripts,
executables, cache links/junctions and unexpected pack files are rejected.
Downloads use temporary files, verified size/hash and a final completion marker.
The cache currently has no eviction UI; stop the game before removing old packs
or clearing this dedicated cache folder. Do not remove local mods or game files.
After a crash, the next preparation removes only recognized random-hash staging
files beside targets in the current inventory while holding the cache writer
lock. Verified blobs can repair interrupted pack files without redownloading.
Unexpected files and links remain errors; they are not silently deleted.

Preparation runs off the render thread; GPU uploads are split across frames.
The session uses its own resource loader and catalogs. Existing local mods are
kept separately in memory and restored on disconnect, including after a failed
login. Joining an unmodified host temporarily uses the original resource set.
Keeping the offline world for restoration increases memory use during a session.

GTA V/FiveM resources must first be converted to supported native assets.
Automatic download does not run Lua, DLL or FiveM scripts. Hosts should share
only resources they may redistribute; each player reads original assets locally.
Hashes detect changed data, not trustworthiness of the host. Selected cars,
peds and clothing now synchronize using the shared ordered catalogs.

Verified with real direct/relay sockets, corrupt/interrupted cache tests, and
two Vulkan runtime instances using different local resources and separate
caches. First join transfers the tiny test pack; rejoin reuses it. Disconnect
restores and renders the offline world. Separate-PC/internet tests remain pending.

## Current synchronization

- Player ground position, facing, idle/walk state and interior ID.
- While driving: vehicle ground position, yaw, pitch, roll and speed.
- Selected original/custom car and ped models, and up to 16 clothing toggles
  per ped. Remote animation uses the selected rig; clothing is applied per
  player without changing your own outfit. Missing model IDs use original
  Grove Street/Taxi fallbacks. Model replacement is limited to one per frame
  and at most once per second per peer.
- Remote movement is smoothed, with immediate changes for teleports,
  entering/exiting vehicles and interior changes. Only actors in your interior
  and within 300 metres are rendered. Remote animation uploads are capped at
  approximately 30 Hz.

Vehicles are currently personal: one car per connected player. After you
spawn/use it, other players see it while driving and after you exit. Its pose
is independent of your walking position/interior; a nearby parked car remains
visible even if its owner moves beyond the avatar rendering range. Both are
culled independently at 300 metres in their own interior. Stationary car meshes
are not uploaded repeatedly. Selecting another car replaces your personal car;
disconnecting removes it. Parked cars are frozen, not independently simulated.

Passenger seats, exchanging vehicles, vehicle/player collisions,
damage, weapons, NPCs spawned through `/peds` and time/weather are
**not synchronized**. Each client simulates their own
movement and collisions. Remote actors have no physical collision. This is a
freeroam connection/replication prototype, not a complete shared simulation.

## Transport and hosting architecture

One player's running game owns membership and relays the latest poses to all
guests. This is a player-hosted star topology, not a full mesh between every
pair of players. Alternatively, `sa-server` hosts 20 actual clients without a
running game; see [dedicated server setup](server-hosting.md).

An optional independent `sa-relay` process supplies discovery and outbound
tunnels. It is separate from the dedicated game server.

The first transport uses TCP with `TCP_NODELAY` and 20 Hz pose/snapshot updates.
Packet loss can delay subsequent TCP snapshots; UDP sequencing/interpolation
and NAT traversal are future work. Socket reads/writes and connection attempts
run on a background thread. Render-thread communication uses bounded,
latest-only mailboxes. Length-prefixed messages support partial TCP reads and
writes; frame sizes, pending connections, output buffers and work per peer are
bounded. Version, identity, name and finite/bounded position checks are applied.
Inactive connections time out after ten seconds. Host identities are assigned
by the host; guests cannot submit another player's ID or a world snapshot.

Sessions currently have no password, encryption or gameplay anti-cheat. Test
with people you trust. Movement remains client-authoritative.

## Launch options and verification

Optional arguments to `start-freeroam.cmd`:

```text
--host 0.0.0.0:7777 --name Host
--join 192.168.1.10:7777 --name Guest
```

`cargo test --manifest-path native/Cargo.toml -p sa-net` exercises real loopback
connections for one host and nineteen clients, pose relay, rejection of a
twenty-first player, freed-slot reuse, host departure, incompatible versions,
forged snapshots, fragmented/coalesced packets and bounded buffers.

On Windows, `tools/test-multiplayer.ps1` runs the two-instance check and saves
logs/captures under `native/target/mp-smoke-<timestamp>`. Add `-Appearance`
to test different outfits and model changes using copies of our own demo
resources; add `-Relay` and/or `-Dedicated` for those hosting modes.

Appearance replication uses network protocol **4**. Update the runtime,
dedicated server and relay together; older protocol versions are rejected.

For a GPU integration check, launch two runtime instances with `--smoke-network`
and complementary `--host 127.0.0.1:17777` / `--join 127.0.0.1:17777` arguments.
Start the host first. The test observes a remote walking ped and a moving car,
then both players exit and their parked cars remain visible. In the scripted
HostTest/ClientTest route, the host also moves 350 metres away while the guest
continues to render the nearby parked car. It then restores the offline world; `--capture-dir <directory>` saves a world screenshot. New guests seek an unoccupied standing position near the Grove Street road
spawn, rather than spawning at the streaming center on a garage/roof.

The network capacity test does not establish engine performance with twenty
fully rendered game clients, nor verify internet/router connectivity. Those
need testing on separate machines and real networks.

Relay launch examples:

```text
--relay-address 127.0.0.1:7778 --relay-host --public-session --name Host
--relay-address 127.0.0.1:7778 --join-code <12-character-code> --name Guest
```

`tools/test-multiplayer.ps1 -Relay` starts its own loopback relay and two Vulkan
runtime instances, reads the host's generated code and joins through the tunnel.
It saves logs/captures alongside the existing direct-mode test. Network tests
cover public/private discovery, code joining, driving poses, twenty-player
capacity, rejected overflow, slot reuse, bad credentials/version/frame sizes,
host withdrawal and relay failure. These are local tests, not an internet test.

Original fallback catalogs contain each original choice once. When a custom
primary model is present, the original Taxi/Grove Street choice remains
available alongside it. Both offline and server preparation use the same catalog
ordering. Protocol 4 rejects older catalog numbering to prevent a selected
vehicle or character from appearing as a different model on another client.
