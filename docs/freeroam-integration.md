# Freeroam integration milestones

Work branch: `feature/shared-freeroam-server`. Launcher design is out of scope.

## Session preparation and atomic entry

The resource worker downloads/verifies the server inventory; GPU uploads remain
incremental. A cancellable connection ticket waits for the gameplay handshake,
and for relay publication when hosting, before replacing the offline world.
Prepared worlds are discarded on bind, handshake, relay or cancellation errors.
All failure paths clear pending automatic entry, session flags and jobs. Offline
checkpoints are not written during preparation, upload or connection waiting.
The original installation is read only.

This is a focused reliability change, not completion of all multiplayer goals.
Shared vehicle ownership/transfer, dedicated administration/authentication and
Lua server resources are still pending. Current personal parked cars still leave
with their owner. The gameplay protocol remains version 6.

## Verification, 8 October 2026

- Workspace tests passed (158 tests before the smoke flag addition); runtime
  tests passed again afterwards (33 tests), including stalled relay handshake
  cancellation and failed host binding. Workspace Clippy passed; runtime Clippy
  passed again after the smoke flag addition. Formatting checked.
- Release runtime, server and relay built in `native/target/freeroam-build`
  while an existing user game held the usual release executable open.
- `tools/test-session-lifecycle.ps1 -BinaryDirectory .\native\target\freeroam-build\release`:
  occupied host port fails **after resource upload**. Runtime assertions verify
  the offline world/catalog/handling remain intact, all connection jobs and flags
  clear, automatic entry is cancelled, and offline play subsequently renders.
- `tools/test-multiplayer.ps1 -Relay -Dedicated -Appearance -Passenger -Audio -AutoPlay -BinaryDirectory .\native\target\freeroam-build\release`:
  two real Vulkan runtimes, dedicated server, local relay, controlled native mods,
  avatar/clothing/car changes, passenger movement/safe exit, parked car visibility,
  sound emitters and offline restoration passed. Screenshots inspected.
- Captures/logs: ignored `native/target/join-failure-20261008-172314` and
  `native/target/mp-smoke-20261008-172200`. Test assets are project-owned demos.
- These are **same-PC loopback** tests on RTX 3070, not LAN/internet verification.
  A user game and the failure smoke also ran during part of the multiplayer test;
  timings are diagnostic observations, not a performance comparison.
  The host log recorded one streamed region in 0.61 s and incremental upload
  over 33 frames (peak CPU upload slice 10.12 ms; soft budget 2 ms).
  Retaining/restoring the offline world used 756.9/690.9 MiB resident memory
  around restoration. These are CPU/working-set figures, not GPU timestamps.

## Two different PCs: acceptance test

Use the same client/server build and protocol version. Each client needs its own
legal original installation. Only native server resources are distributed.

1. On a trusted LAN, bind `sa-server` to its LAN address and TCP port 7777 in
   `server.json`. Allow that port only from the test network in the host firewall.
   On the second PC use **Direct connect** with the host's LAN address, never
   `127.0.0.1`. Start a second client on the first PC if desired.
2. For a private internet test, use a trusted VPN between the PCs/server. The
   current TCP transport is unencrypted. It is not suitable for passwords or
   sensitive resources sent across an untrusted public connection.
3. Relay testing is separate: run `sa-relay <VPN-IP>:7788`, configure the server's
   `relay` address, then use that same reachable relay address and its actual
   join code on both clients. A loopback relay address is only local to one PC.
   Do not publish/deploy a public relay as part of this test.
4. Join with a controlled resource pack; confirm names/models/clothing on both
   screens. Stop one client, join it again, and verify late arrival sees the other
   player and their parked vehicle. Board as passenger, drive across a region
   boundary, leave at a supported road, then stop the host/server.
5. Verify connection failure/timeout returns each client to its own local world,
   local mods, handling and checkpoint. Repeat with an unreachable relay,
   mismatched build, disabled downloads and a changed resource version.
6. Record actual RTT, frame/CPU upload timings (F3), working set, resource logs,
   both client logs and any firewall/VPN configuration. Do not infer ping from
   frame rate. Driver transfer is pending and must not be reported as passing.

The most important next integration is stable server vehicle IDs and driver-seat
ownership, including race tests, late join snapshots and disconnect parking.
