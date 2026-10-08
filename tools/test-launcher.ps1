param([string]$GameDir='E:\GTA San Andreas\Grand Theft Auto San Andreas', [string]$Artwork='')
$ErrorActionPreference='Stop'
$launcherRepo=Split-Path -Parent $PSScriptRoot
$launcherExe=Join-Path $launcherRepo 'native/target/release/sa-launcher.exe'
if (-not (Test-Path -LiteralPath $launcherExe)) {throw 'Build the release launcher first.'}
$launcherRoot=Join-Path $launcherRepo ('native/target/launcher-ui-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Path $launcherRoot -Force | Out-Null
$launcherVariables=@('SARE_CONFIG_DIR','SARE_SCREENSHOT_TO','SARE_PREVIEW_PAGE','SARE_PREVIEW_SCALE','SARE_PREVIEW_COMPACT','SARE_PREVIEW_SIZE')
$launcherOld=@{}
foreach($launcherVariable in $launcherVariables){$launcherOld[$launcherVariable]=[Environment]::GetEnvironmentVariable($launcherVariable,'Process')}
try {
 foreach($launcherCase in @(
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
  @{name='invalid-installation';page='settings';scale='1';size='1280x720';invalid=$true}
 )) {
  $launcherProfile=Join-Path $launcherRoot $launcherCase.name
  New-Item -ItemType Directory -Path $launcherProfile -Force | Out-Null
  $launcherGame=$GameDir
  if($launcherCase.invalid){$launcherGame=Join-Path $launcherProfile 'missing-game'}
  $launcherConfig=@{game_dir=$launcherGame;player='UI test';relay='127.0.0.1:7778';favorites=@()}
  [IO.File]::WriteAllText((Join-Path $launcherProfile 'launcher.json'),($launcherConfig|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
  if($launcherCase.saved){
   if($Artwork){$launcherConfig.hero_image=$Artwork}
   $launcherSave=@{version=1;position=@(2500,-1670,13.5);yaw=0;pitch=0;car='Infernus';ped='CJ';clothes=@()}
   [IO.File]::WriteAllText((Join-Path $launcherProfile 'progress.json'),($launcherSave|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
   [IO.File]::WriteAllText((Join-Path $launcherProfile 'launcher.json'),($launcherConfig|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
  }
  $env:SARE_CONFIG_DIR=$launcherProfile
  $env:SARE_PREVIEW_PAGE=$launcherCase.page
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
 Write-Output ('Eleven native launcher captures passed: '+$launcherRoot)
} finally {
 foreach($launcherVariable in $launcherVariables){[Environment]::SetEnvironmentVariable($launcherVariable,$launcherOld[$launcherVariable],'Process')}
}
