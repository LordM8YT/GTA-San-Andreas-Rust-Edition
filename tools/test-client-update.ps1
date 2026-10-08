param([string]$GameDir='E:\GTA San Andreas\Grand Theft Auto San Andreas')
$ErrorActionPreference='Stop'
$updateRepo=Split-Path -Parent $PSScriptRoot
$updateZip=Join-Path $updateRepo 'native/target/packages/SARE-windows-test.zip'
if(-not(Test-Path -LiteralPath $updateZip)){throw 'Build and package the Windows client first.'}
$updateRoot=Join-Path $updateRepo ('native/target/update-smoke-'+(Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Path $updateRoot -Force | Out-Null
$updateOld=@{}
foreach($updateVariable in @('SARE_CONFIG_DIR','SARE_SCREENSHOT_TO','SARE_PREVIEW_PAGE','SARE_PREVIEW_SCALE','SARE_PREVIEW_SIZE')){$updateOld[$updateVariable]=[Environment]::GetEnvironmentVariable($updateVariable,'Process')}
try {
 foreach($updateCase in @('install','game-running')) {
  $updateClient=Join-Path $updateRoot $updateCase
  $updateStage=Join-Path $updateClient '.sare-update/smoke-stage'
  $updateProfile=Join-Path $updateClient 'test-profile'
  New-Item -ItemType Directory -Path $updateProfile -Force | Out-Null
  Expand-Archive -LiteralPath $updateZip -DestinationPath $updateClient
  Expand-Archive -LiteralPath $updateZip -DestinationPath (Join-Path $updateStage 'new')
  $updateManifest=Get-Content (Join-Path $updateClient 'sare-build.json') -Raw -Encoding UTF8 | ConvertFrom-Json
  $updateExpected=$updateManifest.commit
  $updateManifest.commit='0000000000000000000000000000000000000000'
  [IO.File]::WriteAllText((Join-Path $updateClient 'sare-build.json'),($updateManifest|ConvertTo-Json -Depth 8),[Text.UTF8Encoding]::new($false))
  New-Item -ItemType Directory -Path (Join-Path $updateClient 'mods') -Force | Out-Null
  [IO.File]::WriteAllText((Join-Path $updateClient 'mods/keep.json'),'local mod sentinel')
  [IO.File]::WriteAllText((Join-Path $updateProfile 'settings.json'),'{}')
  $updateConfig=@{game_dir=$GameDir;player='Update test';relay='127.0.0.1:7778';favorites=@();disable_auto_updates=$true}
  [IO.File]::WriteAllText((Join-Path $updateProfile 'launcher.json'),($updateConfig|ConvertTo-Json),[Text.UTF8Encoding]::new($false))
  Copy-Item -LiteralPath (Join-Path $updateClient 'sa-launcher.exe') -Destination (Join-Path $updateStage 'update-worker.exe')
  $env:SARE_CONFIG_DIR=$updateProfile
  $env:SARE_SCREENSHOT_TO=Join-Path $updateProfile 'restarted.png'
  $env:SARE_PREVIEW_PAGE='help'
  $env:SARE_PREVIEW_SCALE='1'
  $env:SARE_PREVIEW_SIZE='1280x720'
  $updateLease=[IO.File]::Open((Join-Path $updateClient '.sare-update/client.lock'),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::ReadWrite)
  $updateLease.Lock(0,1)
  $updateGameLease=$null
  if($updateCase -eq 'game-running'){
   $updateGameLease=[IO.File]::Open((Join-Path $updateProfile 'runtime.lock'),[IO.FileMode]::OpenOrCreate,[IO.FileAccess]::ReadWrite,[IO.FileShare]::ReadWrite)
   $updateGameLease.Lock(0,1)
  }
  try {
   $updateProcess=Start-Process -FilePath (Join-Path $updateStage 'update-worker.exe') -ArgumentList @('--apply-update',('"'+$updateStage+'"')) -WindowStyle Hidden -PassThru -RedirectStandardError (Join-Path $updateProfile 'worker-errors.log')
   $updateProcessHandle=$updateProcess.Handle
   Start-Sleep -Milliseconds 300
   $updateLease.Unlock(0,1)
   $updateLease.Dispose()
   if(-not $updateProcess.WaitForExit(20000)){Stop-Process -Id $updateProcess.Id;throw 'Update worker timeout'}
   for($updateWait=0;$updateWait -lt 100 -and -not(Test-Path -LiteralPath $env:SARE_SCREENSHOT_TO);$updateWait++){Start-Sleep -Milliseconds 200}
   if(-not(Test-Path -LiteralPath $env:SARE_SCREENSHOT_TO)){throw 'Updated launcher did not restart/capture'}
   $updateAfter=Get-Content (Join-Path $updateClient 'sare-build.json') -Raw -Encoding UTF8 | ConvertFrom-Json
   $updateResult=Get-Content (Join-Path $updateClient '.sare-update/last-result.txt') -Raw -Encoding UTF8
   if($updateCase -eq 'install'){
    if($updateAfter.commit -ne $updateExpected -or $updateResult -notmatch 'installed'){throw 'Update was not applied'}
    if(-not(Test-Path -LiteralPath (Join-Path $updateStage 'backup/sa-launcher.exe'))){throw 'Missing previous client backup'}
   } else {
    if($updateAfter.commit -ne '0000000000000000000000000000000000000000' -or $updateResult -notmatch 'already running'){throw 'Running-game protection failed'}
   }
   foreach($updateEntry in $updateAfter.files){
    if((Get-FileHash -LiteralPath (Join-Path $updateClient $updateEntry.path) -Algorithm SHA256).Hash.ToLowerInvariant() -ne $updateEntry.sha256){throw ('Changed installed file: '+$updateEntry.path)}
   }
   if([IO.File]::ReadAllText((Join-Path $updateClient 'mods/keep.json')) -ne 'local mod sentinel'){throw 'Local mod changed'}
   if([IO.File]::ReadAllText((Join-Path $updateProfile 'settings.json')) -ne '{}'){throw 'User settings changed'}
   Write-Output ('Native updater '+$updateCase+' passed: '+$updateClient)
  } finally {
   if($updateLease){$updateLease.Dispose()}
   if($updateGameLease){$updateGameLease.Unlock(0,1);$updateGameLease.Dispose()}
  }
 }
} finally {
 foreach($updateVariable in $updateOld.Keys){[Environment]::SetEnvironmentVariable($updateVariable,$updateOld[$updateVariable],'Process')}
}
