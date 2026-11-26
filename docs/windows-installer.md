# Windows installer

`SARE-Setup.exe` installs SARE Client the way FiveM or RedM install: download,
double-click, click Install, then start SARE from the desktop or Start menu.

Always the newest: <https://github.com/LordM8YT/SARE/releases/latest/download/SARE-Setup.exe>

## What it does

- Installs for the current user only, without an administrator prompt, into
  `%LOCALAPPDATA%\SARE` (open it by typing that path into Explorer).
- Creates a **SARE** shortcut on the desktop and in the Start menu, and offers
  to start SARE when it is done.
- Adds SARE to *Settings › Apps* so it can be uninstalled like any program.
- After installing, the launcher updates itself from GitHub releases exactly
  like an extracted ZIP does ([automatic updates](client-updates.md)). Running
  a newer `SARE-Setup.exe` over an existing install also upgrades it.

## Where things are

| What | Where |
| --- | --- |
| Programs, bundled worlds | `%LOCALAPPDATA%\SARE` |
| Your own worlds (Create) | `%LOCALAPPDATA%\SARE\content` |
| Your own mods | `%LOCALAPPDATA%\SARE\mods` |
| Settings, logs, server cache | `%LOCALAPPDATA%\SAFreeroam` |

Uninstalling removes the programs, the bundled worlds' files and the update
folder. It keeps your own worlds and mods, and the settings and cache in
`%LOCALAPPDATA%\SAFreeroam`; delete those folders by hand for a clean slate.

## Building it

CI builds the installer on the Windows runner from the tested
`SARE-windows-test.zip`, so it holds byte for byte the same files, then
installs it silently into a temporary folder, checks every file against
`sare-build.json`, uninstalls it and checks that only a test world and mod are
left. Locally, with [Inno Setup 6](https://jrsoftware.org/isinfo.php):

    python tools/package-client.py --platform windows
    python tools/build-installer.py --smoke-test

The script is `tools/installer/sare.iss`. Never change its `AppId`: Windows uses
it to recognise an installed SARE when a newer installer runs.

## Not yet

- The installer is not code-signed, so Windows SmartScreen may warn on first
  run (*More info › Run anyway*).
- It bundles the whole client (about 25 MB) instead of downloading it.
- The ZIPs for Windows, Linux and macOS stay as they are.
