param(
    [string]$GameDir = 'E:\GTA San Andreas\Grand Theft Auto San Andreas',
    [string]$BinaryDirectory
)
$ErrorActionPreference = 'Stop'
$lifecycleRepo = Split-Path -Parent $PSScriptRoot
if (-not $BinaryDirectory) { $BinaryDirectory = Join-Path $lifecycleRepo 'native\target\release' }
$lifecycleResults = Join-Path $lifecycleRepo ('native\target\join-failure-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Path $lifecycleResults | Out-Null
$lifecycleRuntime = Join-Path $BinaryDirectory 'sa-runtime.exe'
$lifecycleOccupied = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
$lifecycleOccupied.Start()
$lifecycleAddress = '127.0.0.1:' + $lifecycleOccupied.LocalEndpoint.Port
$lifecycleProcess = $null
try {
    $lifecycleProcess = Start-Process -WindowStyle Hidden -FilePath $lifecycleRuntime -WorkingDirectory $lifecycleRepo -PassThru `
        -ArgumentList @('--game-dir', ('"' + $GameDir + '"'), '--renderer', 'vulkan', '--no-mods', '--host', $lifecycleAddress, '--play', '--smoke-join-failure', '--cache-dir', ('"' + (Join-Path $lifecycleResults 'cache') + '"'), '--capture-dir', ('"' + $lifecycleResults + '"')) `
        -RedirectStandardOutput (Join-Path $lifecycleResults 'runtime.log') -RedirectStandardError (Join-Path $lifecycleResults 'runtime-errors.log')
    $null = $lifecycleProcess.Handle
    if (-not $lifecycleProcess.WaitForExit(120000)) { throw "Failed join smoke timed out. Inspect $lifecycleResults" }
    if ($lifecycleProcess.ExitCode -ne 0) { throw "Failed join runtime exited with $($lifecycleProcess.ExitCode). Inspect $lifecycleResults" }
    $lifecycleLog = Get-Content -LiteralPath (Join-Path $lifecycleResults 'runtime.log') -Raw
    if ($lifecycleLog -notmatch 'GPU failed join smoke passed') { throw "Failed join assertions missing. Inspect $lifecycleResults" }
    if (-not (Test-Path -LiteralPath (Join-Path $lifecycleResults 'failed-join-offline.png'))) { throw 'Offline capture missing.' }
    Write-Output "Failed joining after resource upload retained the offline world and rendered offline play. Results: $lifecycleResults"
} finally {
    $lifecycleOccupied.Stop()
    if ($null -ne $lifecycleProcess -and -not $lifecycleProcess.HasExited) { Stop-Process -Id $lifecycleProcess.Id }
}
