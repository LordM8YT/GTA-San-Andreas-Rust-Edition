# SARE directory: friends and online status

`sa-directory` is the small service behind **Friends** in SARE Client. It is
part of SARE (`native/crates/net/src/directory.rs`), open source like the rest,
and anyone can run one. The client uses the official directory
(`https://sare-directory.onrender.com`, on Render's free plan) unless the
player sets another under **Settings › Friends**, or `SARE_DIRECTORY`.

## How it works

- **No accounts, no passwords, no e-mail.** A player's SARE ID is an Ed25519
  key derived from the secret SARE already keeps in `identity.key`. Its friend
  code (`ABCD-EFGH-JKLM`) is a short hash of the public key. The SARE ID is
  separate from the license a player has on each server, and servers never
  see it.
- About once a minute, an open SARE Client sends a **signed heartbeat**: name,
  what it is playing (world, and the server address if it joined one by IP)
  and the friend codes it has added.
- The directory answers with the friends who are online **and have added the
  player back**. Adding a stranger's code shows nothing until they add yours.
- Everything is kept **in memory only**. Nothing is written to disk; a player
  disappears 150 seconds after their last heartbeat, and a restart forgets
  everyone. There are no logs of who was online.
- Heartbeats with a bad signature, a clock more than five minutes off, more
  than 200 friends or more than 16 KiB are refused. At most 64 requests are
  handled at once.

API: `POST /v1/heartbeat` with `{"payload": "<heartbeat JSON>", "signature":
"<hex>"}`, answered with `{"online": [{"code", "name", "playing"}]}`, and
`GET /health` with `{"online": <count>}`.

## Run one locally

```text
cargo run --release -p sa-net --bin sa-directory -- 127.0.0.1:7780
```

Set **Settings › Friends** to `http://127.0.0.1:7780` (plain `http://` is
only accepted for `127.0.0.1` and `localhost`) and open Friends.

## Host the official directory on Render (free, no card)

The repository has a Render Blueprint (`render.yaml`) that builds
`sa-directory` from `native/crates/net/directory.Dockerfile`. Render gives it
an `https://….onrender.com` address with a certificate, so no VM, domain or
reverse proxy is needed. Without an argument, `sa-directory` listens on
`0.0.0.0:$PORT`, which Render sets.

1. Sign in at [render.com](https://render.com) with **GitHub**. The free plan
   needs no card.
2. *New › Blueprint*, give Render access to the SARE repository and pick it.
   Render reads `render.yaml` and shows one free web service,
   `sare-directory`. Press **Deploy Blueprint**. The first build takes a few
   minutes.
3. **Check**: open `https://sare-directory.onrender.com/health` (Render shows
   the exact address on the service page). It answers `{"online":0}`.
4. **Make it the default**: set `DEFAULT_DIRECTORY` in
   `native/crates/launcher/src/friends.rs` to that address and release a
   client.

A free Render service sleeps after 15 minutes with no requests and takes
about half a minute to wake up; the client waits up to 45 seconds for a
heartbeat, and every open client sends one each minute, so it stays awake
while anyone is online. Sleeping forgets everyone, which is fine: players
reappear with their next heartbeat. To update, press *Manual Deploy* on the
service page (`autoDeploy` is off so pushes to `main` do not restart it).

The same container runs anywhere Docker does:

```text
cd native
docker build -f crates/net/directory.Dockerfile -t sa-directory .
docker run -p 7780:7780 sa-directory
```

## Host it on Oracle Cloud Always Free (a VM of your own)

An alternative to Render that never sleeps, at the cost of running a VM.

The service speaks plain HTTP on a local port. A reverse proxy in front of it
adds HTTPS; Caddy does that with a free Let's Encrypt certificate and no
extra setup.

1. **Create the account** at [oracle.com/cloud/free](https://www.oracle.com/cloud/free/).
   It asks for a card to verify you; Always Free resources are not charged.
   Pick a home region close to your players (for example Stockholm or
   Frankfurt); it cannot be changed later.
2. **Create a VM**: *Compute › Instances › Create instance*. Image *Ubuntu
   24.04*, shape *VM.Standard.A1.Flex* (Ampere, Always Free eligible) with
   1 OCPU and 6 GB memory, which is plenty. Download the SSH key it offers.
3. **Open HTTPS**: in the instance's *Virtual cloud network › Security list*,
   add ingress rules for TCP 80 and 443 from `0.0.0.0/0`. On the VM:

   ```bash
   sudo iptables -I INPUT -p tcp --dport 80 -j ACCEPT
   sudo iptables -I INPUT -p tcp --dport 443 -j ACCEPT
   sudo netfilter-persistent save
   ```

4. **A name**: point a DNS name at the VM's public IP (an `A` record such as
   `directory.example.org`). A free name from a dynamic DNS service works too.
5. **Build and install** on the VM:

   ```bash
   sudo apt update && sudo apt install -y build-essential git caddy
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
   . ~/.cargo/env
   git clone https://github.com/LordM8YT/SARE.git && cd SARE/native
   cargo build --release -p sa-net --bin sa-directory
   sudo install target/release/sa-directory /usr/local/bin/
   ```

6. **Run it as a service**: save as `/etc/systemd/system/sa-directory.service`

   ```ini
   [Unit]
   Description=SARE directory
   After=network.target

   [Service]
   ExecStart=/usr/local/bin/sa-directory 127.0.0.1:7780
   DynamicUser=yes
   Restart=always

   [Install]
   WantedBy=multi-user.target
   ```

   then `sudo systemctl enable --now sa-directory`.
7. **HTTPS**: replace `/etc/caddy/Caddyfile` with

   ```text
   directory.example.org {
       reverse_proxy 127.0.0.1:7780
   }
   ```

   and run `sudo systemctl reload caddy`.
8. **Check** from any PC: `https://directory.example.org/health` shows
   `{"online":0}`.
9. **Make it the default**: set `DEFAULT_DIRECTORY` in
   `native/crates/launcher/src/friends.rs` to the `https://` address and
   release a client.

To update: `git pull`, build again, `sudo install ...` and
`sudo systemctl restart sa-directory`. Players reappear within a minute.
