param([string]$GameDir = 'E:\GTA San Andreas\Grand Theft Auto San Andreas')
$ErrorActionPreference = 'Stop'
$saveRepo = Split-Path -Parent $PSScriptRoot
$saveResults = Join-Path $saveRepo ('native\target\progress-smoke-' + (Get-Date -Format yyyyMMdd-HHmmss))
New-Item -ItemType Directory -Path $saveResults | Out-Null
$saveExe = Join-Path $saveResults sa-runtime-save.exe
Copy-Item (Join-Path $saveRepo native/target/release/sa-runtime.exe) $saveExe
$saveMods = Join-Path $saveResults mods
New-Item -ItemType Directory -Path $saveMods | Out-Null
foreach ($saveDemo in @('native-car-demo','native-clothing-demo')) {
    Copy-Item -LiteralPath (Join-Path $saveRepo ('mods/' + $saveDemo)) -Destination $saveMods -Recurse
    $saveManifest = Join-Path $saveMods ($saveDemo + '/mod.json')
    $saveData = Get-Content -LiteralPath $saveManifest -Raw -Encoding UTF8 | ConvertFrom-Json
    $saveData.enabled = $true
    [System.IO.File]::WriteAllText($saveManifest, ($saveData | ConvertTo-Json -Depth 16), [System.Text.UTF8Encoding]::new($false))
}
$saveFile = Join-Path $saveResults progress.json
foreach ($saveStage in @('first','restored','without-mods','unsafe')) {
    if ($saveStage -eq 'unsafe') {
        $saveData = Get-Content -LiteralPath $saveFile -Raw -Encoding UTF8 | ConvertFrom-Json
        $saveData.position[2] = 1999.0
        [System.IO.File]::WriteAllText($saveFile, ($saveData | ConvertTo-Json -Depth 16), [System.Text.UTF8Encoding]::new($false))
    }
    $saveCapture = Join-Path $saveResults $saveStage
    New-Item -ItemType Directory -Path $saveCapture | Out-Null
    $saveArgs = @('--game-dir', ('"' + $GameDir + '"'), '--renderer','vulkan', '--smoke-save',
        '--save-file', ('"' + $saveFile + '"'), '--capture-dir', ('"' + $saveCapture + '"'))
    if ($saveStage -in @('first','restored')) { $saveArgs += @('--mods-dir', ('"' + $saveMods + '"')) }
    else { $saveArgs += '--no-mods' }
    $saveProcess = Start-Process -WindowStyle Hidden -FilePath $saveExe -WorkingDirectory $saveResults -ArgumentList $saveArgs -PassThru `
        -RedirectStandardOutput (Join-Path $saveResults ($saveStage+'.log')) -RedirectStandardError (Join-Path $saveResults ($saveStage+'-errors.log'))
    $saveProcessHandle=$saveProcess.Handle
    try {
        if (-not $saveProcess.WaitForExit(60000)) { throw "Checkpoint test timed out: $saveStage" }
        if ($saveProcess.ExitCode -ne 0) { throw "Checkpoint test failed: $saveStage ($($saveProcess.ExitCode))" }
        $saveLog = Get-Content (Join-Path $saveResults ($saveStage+'.log')) -Raw
        if ($saveLog -notmatch 'GPU offline checkpoint smoke passed') { throw "Missing checkpoint result: $saveStage" }
        Copy-Item -LiteralPath $saveFile -Destination (Join-Path $saveResults ($saveStage+'.json'))
    } finally { if (-not $saveProcess.HasExited) { Stop-Process -Id $saveProcess.Id } }
}
$saveFirst = Get-Content (Join-Path $saveResults first.json) -Raw
if ($saveFirst -ne (Get-Content (Join-Path $saveResults restored.json) -Raw)) { throw 'Restored position/model/clothing changed' }
$saveRestored = $saveFirst | ConvertFrom-Json
if ($saveRestored.clothes.Count -lt 2 -or $saveRestored.clothes[0][1]) { throw 'Clothing selection did not persist' }
$saveMissing = Get-Content (Join-Path $saveResults without-mods.json) -Raw | ConvertFrom-Json
if ($saveMissing.clothes.Count -ne 0 -or $saveMissing.ped -ne 'Grove Street') { throw 'Missing ped did not fall back to original player' }
if (($saveMissing.position | ConvertTo-Json -Compress) -ne ($saveRestored.position | ConvertTo-Json -Compress)) { throw 'Removing the mod changed a safe position' }
$saveUnsafeLog = Get-Content (Join-Path $saveResults unsafe-errors.log) -Raw
if ($saveUnsafeLog -notmatch 'Saved location has no safe support') { throw 'Unsafe position was not rejected' }
$saveUnsafe = Get-Content (Join-Path $saveResults unsafe.json) -Raw | ConvertFrom-Json
if ($saveUnsafe.position[2] -ge 100.0) { throw 'Unsafe saved height was restored' }
Write-Output "Four-start Vulkan checkpoint smoke passed. Results: $saveResults"
