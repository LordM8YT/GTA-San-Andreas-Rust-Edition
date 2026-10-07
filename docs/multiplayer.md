# Player-hosted multiplayer prototype

Open **Multiplayer** from the main/pause menu, press **F5** in free roam, or
type **/mp**. Set your name, then choose **Host session** or **Join session**.
The limit is **20 total players: one host and 19 guests**.

The host normally listens on `0.0.0.0:7777`. Guests enter the host's actual IP,
for example `192.168.1.10:7777`; do not join `0.0.0.0`. On the same computer,
use `127.0.0.1:7777`. For an internet connection the host needs an accessible
public address, a firewall rule allowing the application, and TCP port 7777
forwarded to their computer. CGNAT can prevent direct hosting; a shared VPN
network is another option. There is no automatic NAT traversal, lobby service,
UPnP, matchmaking or relay service in this version.

Enter free roam after connecting. The session page shows the connection status
and player list; gameplay shows the player count. Opening a menu pauses your
own movement while the network and other players continue. **Disconnect**
leaves the session. Returning to the main menu also disconnects. If the host
quits, guests return to an offline world and can reconnect through Multiplayer.
There is no host migration.

Each player needs their own original San Andreas installation and this runtime.
The host does not distribute game files or mods. Use compatible local map/mod
sets for testing; there is no asset comparison/download handshake yet.

## Current synchronization

- Player ground position, facing, idle/walk state and interior ID.
- While driving: vehicle ground position, yaw, pitch, roll and speed.
- Other players appear as the original Grove Street ped and Taxi. The local
  player can still choose custom models; their custom appearance is not sent.
- Remote movement is smoothed, with immediate changes for teleports,
  entering/exiting vehicles and interior changes. Only actors in your interior
  and within 300 metres are rendered. Remote animation uploads are capped at
  approximately 30 Hz.

Vehicles are currently personal: other players see your car while you drive.
Parked cars, passenger seats, exchanging vehicles, vehicle/player collisions,
damage, weapons, NPCs spawned through `/peds`, clothing, time/weather and custom
resource replication are **not synchronized**. Each client simulates their own
movement and collisions. Remote actors have no physical collision. This is a
freeroam connection/replication prototype, not a complete shared simulation.

## Transport and hosting architecture

One player's running game owns membership and relays the latest poses to all
guests. This is a player-hosted star topology, not a full mesh between every
pair of players. Separate dedicated server files are not required or supplied.
The independent `sa-net` crate can be reused when dedicated hosting is added.

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
logs/captures under `native/target/mp-smoke-<timestamp>`.

For a GPU integration check, launch two runtime instances with `--smoke-network`
and complementary `--host 127.0.0.1:17777` / `--join 127.0.0.1:17777` arguments.
Start the host first. The test observes a remote walking ped and a moving car,
then exits; `--capture-dir <directory>` saves a world screenshot. The second
instance starts six metres away so the two actors can be distinguished.

The network capacity test does not establish engine performance with twenty
fully rendered game clients, nor verify internet/router connectivity. Those
need testing on separate machines and real networks.
