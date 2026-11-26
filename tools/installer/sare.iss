; SARE Client installer for Windows (Inno Setup 6).
; Built by tools/build-installer.py from the tested client ZIP; do not run
; ISCC on this file by hand without the /D values that script passes.
;
; Per-user install (no administrator prompt) into %LOCALAPPDATA%\SARE, so the
; launcher's own updater can replace files there afterwards. Settings, logs
; and caches stay in %LOCALAPPDATA%\SAFreeroam and survive an uninstall.

#ifndef SourceDir
  #error SourceDir is required
#endif
#ifndef OutputDir
  #error OutputDir is required
#endif
#ifndef AppVersion
  #error AppVersion is required
#endif
#ifndef NumericVersion
  #error NumericVersion is required
#endif

[Setup]
; Never change AppId: it is how Windows knows a new installer upgrades SARE.
AppId={{A8742781-5671-4669-AFDE-323B4BCC8EAF}
AppName=SARE
AppVersion={#AppVersion}
AppVerName=SARE {#AppVersion}
AppPublisher=SARE community
AppPublisherURL=https://github.com/LordM8YT/SARE
AppSupportURL=https://discord.gg/F8KwXJqw6D
AppUpdatesURL=https://github.com/LordM8YT/SARE/releases/latest
VersionInfoVersion={#NumericVersion}
DefaultDirName={localappdata}\SARE
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir={#OutputDir}
OutputBaseFilename=SARE-Setup
SetupIconFile=..\..\native\assets\sare.ico
UninstallDisplayIcon={app}\sa-launcher.exe
UninstallDisplayName=SARE
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
; Close a running SARE before files are replaced or removed.
CloseApplications=yes
RestartApplications=no

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\SARE"; Filename: "{app}\sa-launcher.exe"; WorkingDir: "{app}"
Name: "{autodesktop}\SARE"; Filename: "{app}\sa-launcher.exe"; WorkingDir: "{app}"

[Run]
Filename: "{app}\sa-launcher.exe"; WorkingDir: "{app}"; Description: "Start SARE"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
; Files the launcher's updater or the game added after installation.
Type: filesandordirs; Name: "{app}\.sare-update"
Type: filesandordirs; Name: "{app}\licenses"
Type: files; Name: "{app}\sa-mcp.exe"
Type: files; Name: "{app}\*.log"
Type: files; Name: "{app}\sare-build.json"
; mods\ and content\ hold the player's own mods and worlds (Create saves new
; worlds in content\): they are removed only when empty.
Type: dirifempty; Name: "{app}\mods"
Type: dirifempty; Name: "{app}\content"
Type: dirifempty; Name: "{app}"
