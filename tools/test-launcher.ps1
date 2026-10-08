param([string]$GameDir='E:\GTA San Andreas\Grand Theft Auto San Andreas')
$ErrorActionPreference='Stop'
$launcherRepo=Split-Path -Parent $PSScriptRoot
$launcherExe=Join-Path $launcherRepo 'native/target/release/sa-launcher.exe'
if (-not (Test-Path -LiteralPath $launcherExe)) {throw 'Build the release launcher first.'}
$launcherRoot=Join-Path $launcherRepo ('native/target/launcher-ui-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Path $launcherRoot -Force | Out-Null
$launcherVariables=@('SARE_CONFIG_DIR','SARE_SCREENSHOT_TO','SARE_PREVIEW_PAGE','SARE_PREVIEW_SCALE','SARE_PREVIEW_COMPACT')
$launcherOld=@{}
foreach($launcherVariable in $launcherVariables){$launcherOld[$launcherVariable]=[Environment]::GetEnvironmentVariable($launcherVariable,'Process')}
try {
 foreach($launcherCase in @(
  @{name='home';page='home';scale='1.25'},
  @{name='settings';page='settings';scale='1.25'},
  @{name='multiplayer';page='multiplayer';scale='1.25'},
  @{name='resources';page='resources';scale='1.25'},
  @{name='help';page='help';scale='1.25'},
  @{name='settings-compact-dpi2';page='settings';scale='2';compact=$true},
  @{name='home-compact';page='home';scale='1';compact=$true},
  @{name='invalid-installation';page='settings';scale='1.25';invalid=$true}
 )) {
  $launcherProfile=Join-Path $launcherRoot $launcherCase.name
  New-Item -ItemType Directory -Path $launcherProfile -Force | Out-Null
  $launcherGame=$GameDir
  if($launcherCase.invalid){$launcherGame=Join-Path $launcherProfile 'missing-game'}
  $launcherConfig=@{game_dir=$launcherGame;player='UI test';relay='127.0.0.1:7778';favorites=@()}
  [IO.File]::WriteAllText((Join-Path $launcherProfile 'launcher.json'),($launcherConfig|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
  $env:SARE_CONFIG_DIR=$launcherProfile
  $env:SARE_PREVIEW_PAGE=$launcherCase.page
  $env:SARE_PREVIEW_SCALE=$launcherCase.scale
  $env:SARE_SCREENSHOT_TO=Join-Path $launcherProfile 'preview.png'
  [Environment]::SetEnvironmentVariable('SARE_PREVIEW_COMPACT',$(if($launcherCase.compact){'1'}else{$null}),'Process')
  $launcherProcess=Start-Process -FilePath $launcherExe -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $launcherProfile 'errors.log')
  $launcherProcessHandle=$launcherProcess.Handle
  if(-not $launcherProcess.WaitForExit(25000)){Stop-Process -Id $launcherProcess.Id;throw ('Launcher preview timeout: '+$launcherCase.name)}
  $launcherProcess.Refresh()
  if($launcherProcess.ExitCode -ne 0 -or -not(Test-Path -LiteralPath $env:SARE_SCREENSHOT_TO)){throw ('Launcher preview failed: '+$launcherCase.name)}
  if((Get-Item -LiteralPath $env:SARE_SCREENSHOT_TO).Length -lt 1000){throw 'Empty preview'}
 }
 Write-Output ('Eight native launcher captures passed: '+$launcherRoot)
} finally {
 foreach($launcherVariable in $launcherVariables){[Environment]::SetEnvironmentVariable($launcherVariable,$launcherOld[$launcherVariable],'Process')}
}
