param(
    [string]$GameDir = 'E:\GTA San Andreas\Grand Theft Auto San Andreas',
    [ValidateRange(1,128)][int]$Count = 32,
    [ValidateSet(2,4)][int]$Scale = 2,
    [float]$X = 2500,
    [float]$Y = -1670,
    [string]$OutputDirectory,
    [switch]$InstallEngine
)
$ErrorActionPreference = 'Stop'
$upscaleRepo = Split-Path -Parent $PSScriptRoot
$upscaleTools = Join-Path $upscaleRepo 'private-assets\realesrgan'
$upscaleEngine = Join-Path $upscaleTools 'realesrgan-ncnn-vulkan.exe'
if (-not $OutputDirectory) { $OutputDirectory = Join-Path $upscaleRepo 'mods\local-upscaled-textures' }
$OutputDirectory = [System.IO.Path]::GetFullPath($OutputDirectory)
if (Test-Path -LiteralPath (Join-Path $OutputDirectory 'mod.json')) { throw 'Preview already exists. Choose another OutputDirectory or toggle enabled in its mod.json.' }
if (-not (Test-Path -LiteralPath $upscaleEngine)) {
    if (-not $InstallEngine) { throw 'Run with -InstallEngine to download the official portable Real-ESRGAN Vulkan tool.' }
    New-Item -ItemType Directory -Path $upscaleTools -Force | Out-Null
    $upscaleZip = Join-Path $upscaleTools 'engine.zip'
    Invoke-WebRequest -Uri 'https://github.com/xinntao/Real-ESRGAN/releases/download/v0.2.5.0/realesrgan-ncnn-vulkan-20220424-windows.zip' -OutFile $upscaleZip
    if ((Get-FileHash -LiteralPath $upscaleZip -Algorithm SHA256).Hash -ne 'ABC02804E17982A3BE33675E4D471E91EA374E65B70167ABC09E31ACB412802D') { throw 'Engine download checksum mismatch.' }
    Expand-Archive -LiteralPath $upscaleZip -DestinationPath $upscaleTools -Force
}
$upscaleCulture = [System.Globalization.CultureInfo]::InvariantCulture
Push-Location (Join-Path $upscaleRepo 'native')
try {
    & cargo run --release -p sa-scene --example upscale-textures -- $GameDir $OutputDirectory $upscaleEngine $Count $Scale $X.ToString($upscaleCulture) $Y.ToString($upscaleCulture)
    if ($LASTEXITCODE -ne 0) { throw 'Texture preview generation failed. Original game files were not modified.' }
} finally { Pop-Location }
