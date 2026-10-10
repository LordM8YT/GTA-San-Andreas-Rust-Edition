param([string]$GameDir='E:\GTA San Andreas\Grand Theft Auto San Andreas', [string]$Artwork='')
$ErrorActionPreference='Stop'
$launcherRepo=Split-Path -Parent $PSScriptRoot
$launcherExe=Join-Path $launcherRepo 'native/target/release/sa-launcher.exe'
if (-not (Test-Path -LiteralPath $launcherExe)) {throw 'Build the release launcher first.'}
$launcherRoot=Join-Path $launcherRepo ('native/target/launcher-ui-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Path $launcherRoot -Force | Out-Null
$launcherVariables=@('SARE_CONFIG_DIR','SARE_SCREENSHOT_TO','SARE_PREVIEW_PAGE','SARE_PREVIEW_SCALE','SARE_PREVIEW_COMPACT','SARE_PREVIEW_SIZE','SARE_PREVIEW_RESOURCE_TAB','SARE_PREVIEW_DIRECT','SARE_PREVIEW_FAVORITES')
$launcherOld=@{}
foreach($launcherVariable in $launcherVariables){$launcherOld[$launcherVariable]=[Environment]::GetEnvironmentVariable($launcherVariable,'Process')}
$launcherRelayProcess=$null
$launcherHostProcess=$null
try {
 $launcherRelayExe=Join-Path $launcherRepo 'native/target/release/sa-relay.exe'
 $launcherServerExe=Join-Path $launcherRepo 'native/target/release/sa-server.exe'
 $launcherRelayLog=Join-Path $launcherRoot 'relay.log'
 $launcherRelayProcess=Start-Process -FilePath $launcherRelayExe -ArgumentList '127.0.0.1:0' -WindowStyle Hidden -PassThru -RedirectStandardOutput $launcherRelayLog -RedirectStandardError (Join-Path $launcherRoot 'relay-error.log')
 $launcherDeadline=(Get-Date).AddSeconds(10)
 do {
  Start-Sleep -Milliseconds 50
  $launcherRelayText=Get-Content -LiteralPath $launcherRelayLog -Raw -ErrorAction SilentlyContinue
  if((Get-Date) -gt $launcherDeadline){throw 'Test relay did not start'}
 } until($launcherRelayText -match 'listening on (127[.]0[.]0[.]1:\d+)')
 $launcherRelayAddress=$Matches[1]
 $launcherCases=@(
  @{name='home-1280';page='home';scale='1';size='1280x720'},
  @{name='home-1920';page='home';scale='1';size='1920x1080';saved=$true},
  @{name='home-ultrawide';page='home';scale='1';size='2560x1080';saved=$true},
  @{name='home-dpi2';page='home';scale='2';size='960x540';saved=$true},
  @{name='settings';page='settings';scale='1';size='1280x720'},
  @{name='multiplayer';page='multiplayer';scale='1';size='1280x720'},
  @{name='resources';page='resources';scale='1';size='1280x720'},
  @{name='help';page='help';scale='1';size='1280x720'},
  @{name='settings-compact-dpi2';page='settings';scale='2';compact=$true},
  @{name='home-compact';page='home';scale='1';compact=$true},
  @{name='invalid-installation';page='settings';scale='1';size='1280x720';invalid=$true},
  @{name='servers-empty';page='multiplayer';scale='1';size='1280x720';relay=$launcherRelayAddress},
  @{name='servers-live';page='multiplayer';scale='1';size='1280x720';relay=$launcherRelayAddress;host=$true},
  @{name='servers-favorites';page='multiplayer';scale='1';size='1280x720';relay=$launcherRelayAddress;favorites=$true},
  @{name='servers-no-relay';page='multiplayer';scale='1';size='1280x720';relay=''},
  @{name='servers-direct';page='multiplayer';scale='1';size='1280x720';relay='';direct=$true},
  @{name='server-resources';page='resources';scale='1';size='1280x720';tab='1'},
  @{name='download-cache';page='resources';scale='1';size='1280x720';tab='2'}
 )
 foreach($launcherPage in @('multiplayer','resources','settings','help')) {
  $launcherCases+=@{name=($launcherPage+'-1920');page=$launcherPage;scale='1';size='1920x1080'}
  $launcherCases+=@{name=($launcherPage+'-ultrawide');page=$launcherPage;scale='1';size='2560x1080'}
  $launcherCases+=@{name=($launcherPage+'-dpi2');page=$launcherPage;scale='2';size='960x540'}
 }
 foreach($launcherCase in $launcherCases) {
  if($launcherCase.host){
   $launcherHostData=Join-Path $launcherRoot 'server-data'
   [void][IO.Directory]::CreateDirectory((Join-Path $launcherHostData 'resources'))
   [IO.File]::WriteAllLines((Join-Path $launcherHostData 'server.cfg'),[string[]]@('endpoint_add_tcp "127.0.0.1:0"','sv_hostname "Local test session"',('set sv_relay "'+$launcherRelayAddress+'"')),[Text.UTF8Encoding]::new($false))
   $launcherHostLog=Join-Path $launcherRoot 'host.log'
   $launcherHostProcess=Start-Process -FilePath $launcherServerExe -WorkingDirectory $launcherHostData -ArgumentList @('+exec','server.cfg') -WindowStyle Hidden -PassThru -RedirectStandardOutput $launcherHostLog -RedirectStandardError (Join-Path $launcherRoot 'host-error.log')
   $launcherDeadline=(Get-Date).AddSeconds(10)
   do {
    Start-Sleep -Milliseconds 50
    $launcherHostText=Get-Content -LiteralPath $launcherHostLog -Raw -ErrorAction SilentlyContinue
    if((Get-Date) -gt $launcherDeadline){throw 'Test host did not publish'}
   } until($launcherHostText -match 'join code: [A-F0-9]{12}')
  }
  $launcherProfile=Join-Path $launcherRoot $launcherCase.name
  New-Item -ItemType Directory -Path $launcherProfile -Force | Out-Null
  $launcherGame=$GameDir
  if($launcherCase.invalid){$launcherGame=Join-Path $launcherProfile 'missing-game'}
  $launcherConfig=@{game_dir=$launcherGame;player='UI test';relay='127.0.0.1:1';favorites=@()}
  if($launcherCase.ContainsKey('relay')){$launcherConfig.relay=$launcherCase.relay}
  if($launcherCase.favorites){
   if($launcherHostText -notmatch 'join code: ([A-F0-9]{12})'){throw 'Missing real test server code'}
   $launcherConfig.favorites=@(@{name='Local test session';address=$launcherRelayAddress;relay=$true;code=$Matches[1]})
  }
  [IO.File]::WriteAllText((Join-Path $launcherProfile 'launcher.json'),($launcherConfig|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
  if($launcherCase.saved){
   if($Artwork){$launcherConfig.hero_image=$Artwork}
   $launcherSave=@{version=1;position=@(2500,-1670,13.5);yaw=0;pitch=0;car='Infernus';ped='CJ';clothes=@()}
   [IO.File]::WriteAllText((Join-Path $launcherProfile 'progress.json'),($launcherSave|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
   [IO.File]::WriteAllText((Join-Path $launcherProfile 'launcher.json'),($launcherConfig|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
  }
  $env:SARE_CONFIG_DIR=$launcherProfile
  $env:SARE_PREVIEW_PAGE=$launcherCase.page
  [Environment]::SetEnvironmentVariable('SARE_PREVIEW_FAVORITES',$(if($launcherCase.favorites){'1'}else{$null}),'Process')
  [Environment]::SetEnvironmentVariable('SARE_PREVIEW_RESOURCE_TAB',$launcherCase.tab,'Process')
  [Environment]::SetEnvironmentVariable('SARE_PREVIEW_DIRECT',$(if($launcherCase.direct){'1'}else{$null}),'Process')
  $env:SARE_PREVIEW_SCALE=$launcherCase.scale
  [Environment]::SetEnvironmentVariable('SARE_PREVIEW_SIZE',$launcherCase.size,'Process')
  $env:SARE_SCREENSHOT_TO=Join-Path $launcherProfile 'preview.png'
  [Environment]::SetEnvironmentVariable('SARE_PREVIEW_COMPACT',$(if($launcherCase.compact){'1'}else{$null}),'Process')
  $launcherProcess=Start-Process -FilePath $launcherExe -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $launcherProfile 'errors.log')
  $launcherProcessHandle=$launcherProcess.Handle
  if(-not $launcherProcess.WaitForExit(25000)){Stop-Process -Id $launcherProcess.Id;throw ('Launcher preview timeout: '+$launcherCase.name)}
  $launcherProcess.Refresh()
  if($launcherProcess.ExitCode -ne 0 -or -not(Test-Path -LiteralPath $env:SARE_SCREENSHOT_TO)){throw ('Launcher preview failed: '+$launcherCase.name)}
  if((Get-Item -LiteralPath $env:SARE_SCREENSHOT_TO).Length -lt 1000){throw 'Empty preview'}
  $launcherPng=[IO.File]::ReadAllBytes($env:SARE_SCREENSHOT_TO)
  $launcherWidth=([int]$launcherPng[16] -shl 24) -bor ([int]$launcherPng[17] -shl 16) -bor ([int]$launcherPng[18] -shl 8) -bor [int]$launcherPng[19]
  $launcherHeight=([int]$launcherPng[20] -shl 24) -bor ([int]$launcherPng[21] -shl 16) -bor ([int]$launcherPng[22] -shl 8) -bor [int]$launcherPng[23]
  $launcherSize=if($launcherCase.size){$launcherCase.size.Split('x')}else{@('640','480')}
  if($launcherWidth -ne ([int]$launcherSize[0]*[double]$launcherCase.scale) -or $launcherHeight -ne ([int]$launcherSize[1]*[double]$launcherCase.scale)){throw ('Unexpected capture dimensions: '+$launcherCase.name+' '+$launcherWidth+'x'+$launcherHeight)}
 }
 Write-Output ([string]$launcherCases.Count+' native launcher captures passed: '+$launcherRoot)
} finally {
 if($launcherHostProcess -and -not $launcherHostProcess.HasExited){Stop-Process -Id $launcherHostProcess.Id}
 if($launcherRelayProcess -and -not $launcherRelayProcess.HasExited){Stop-Process -Id $launcherRelayProcess.Id}
 foreach($launcherVariable in $launcherVariables){[Environment]::SetEnvironmentVariable($launcherVariable,$launcherOld[$launcherVariable],'Process')}
}
