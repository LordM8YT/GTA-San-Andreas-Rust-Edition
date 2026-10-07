param(
    [string]$GameDir = 'E:\GTA San Andreas\Grand Theft Auto San Andreas',
    [switch]$Relay
)
$ErrorActionPreference = 'Stop'
$mpRepo = Split-Path -Parent $PSScriptRoot
$mpSource = Join-Path $mpRepo 'native\target\release\sa-runtime.exe'
if (-not (Test-Path -LiteralPath $mpSource)) { throw 'Build the release runtime first.' }
$mpResults = Join-Path $mpRepo ('native\target\mp-smoke-' + (Get-Date -Format 'yyyyMMdd-HHmmss'))
New-Item -ItemType Directory -Path $mpResults -Force | Out-Null
$mpExe = Join-Path $mpResults 'sa-runtime-mp.exe'
Copy-Item -LiteralPath $mpSource -Destination $mpExe
$mpPortProbe = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, 0)
$mpPortProbe.Start()
$mpEndpoint = '127.0.0.1:' + $mpPortProbe.LocalEndpoint.Port
$mpPortProbe.Stop()
$mpHost = $null
$mpClient = $null
$mpRelay = $null
try {
    $mpCommon = @('--game-dir', ('"' + $GameDir + '"'), '--renderer', 'vulkan', '--smoke-network')
    $mpHostArgs = @('--host', $mpEndpoint)
    $mpJoinArgs = @('--join', $mpEndpoint)
    if ($Relay) {
        $mpRelaySource = Join-Path $mpRepo 'native\target\release\sa-relay.exe'
        if (-not (Test-Path -LiteralPath $mpRelaySource)) { throw 'Build sa-relay in release mode first.' }
        $mpRelayExe = Join-Path $mpResults 'sa-relay-test.exe'
        Copy-Item -LiteralPath $mpRelaySource -Destination $mpRelayExe
        $mpRelay = Start-Process -WindowStyle Hidden -FilePath $mpRelayExe -WorkingDirectory $mpRepo -PassThru `
            -ArgumentList @($mpEndpoint) -RedirectStandardOutput (Join-Path $mpResults 'relay.log') -RedirectStandardError (Join-Path $mpResults 'relay-errors.log')
        $mpRelayDeadline = (Get-Date).AddSeconds(10)
        while ((Get-Date) -lt $mpRelayDeadline -and -not $mpRelay.HasExited) {
            if ((Get-Content (Join-Path $mpResults 'relay.log') -Raw) -match 'listening on') { break }
            Start-Sleep -Milliseconds 100
        }
        if ($mpRelay.HasExited -or (Get-Date) -ge $mpRelayDeadline) { throw 'Relay failed to start.' }
        $mpHostArgs = @('--relay-address', $mpEndpoint, '--relay-host', '--public-session')
    }
    $mpHost = Start-Process -WindowStyle Hidden -FilePath $mpExe -WorkingDirectory $mpRepo -PassThru `
        -ArgumentList ($mpCommon + $mpHostArgs + @('--name', 'HostTest', '--capture-dir', ('"' + (Join-Path $mpResults 'host') + '"'))) `
        -RedirectStandardOutput (Join-Path $mpResults 'host.log') -RedirectStandardError (Join-Path $mpResults 'host-errors.log')
    $mpDeadline = (Get-Date).AddSeconds(20)
    while ((Get-Date) -lt $mpDeadline -and -not $mpHost.HasExited) {
        $mpHostErrors = Get-Content (Join-Path $mpResults 'host-errors.log') -Raw
        if ($Relay) {
            if ($mpHostErrors -match 'Multiplayer join code: ([A-F0-9]{12})') {
                $mpJoinArgs = @('--relay-address', $mpEndpoint, '--join-code', $Matches[1])
                break
            }
        } elseif ($mpHostErrors -match 'Multiplayer host listening') { break }
        Start-Sleep -Milliseconds 100
    }
    if ($mpHost.HasExited -or (Get-Date) -ge $mpDeadline) { throw 'Host failed to start. Inspect host-errors.log.' }
    $mpClient = Start-Process -WindowStyle Hidden -FilePath $mpExe -WorkingDirectory $mpRepo -PassThru `
        -ArgumentList ($mpCommon + $mpJoinArgs + @('--name', 'ClientTest', '--capture-dir', ('"' + (Join-Path $mpResults 'client') + '"'))) `
        -RedirectStandardOutput (Join-Path $mpResults 'client.log') -RedirectStandardError (Join-Path $mpResults 'client-errors.log')
    if (-not $mpHost.WaitForExit(45000)) { throw 'Host smoke test timed out.' }
    if (-not $mpClient.WaitForExit(10000)) { throw 'Client smoke test timed out.' }
    foreach ($mpRole in @('host', 'client')) {
        $mpLog = Get-Content (Join-Path $mpResults ($mpRole + '.log')) -Raw
        if ($mpLog -notmatch 'GPU multiplayer smoke passed:.*2 players seen') { throw "$mpRole failed. Inspect logs in $mpResults" }
        if (-not (Test-Path -LiteralPath (Join-Path $mpResults ($mpRole + '\multiplayer-world.png')))) { throw "$mpRole capture missing." }
    }
    Write-Output "Two-instance Vulkan multiplayer smoke passed. Results: $mpResults"
} finally {
    # Only terminate processes created by this script.
    foreach ($mpProcess in @($mpHost, $mpClient, $mpRelay)) {
        if ($null -ne $mpProcess -and -not $mpProcess.HasExited) { Stop-Process -Id $mpProcess.Id }
    }
}
