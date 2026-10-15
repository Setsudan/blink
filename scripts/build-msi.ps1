# Build Blink release binary and produce a WiX MSI under target\wix\.
# Prerequisites: Rust toolchain, cargo-wix (`cargo install cargo-wix`).
# WiX Toolset 3.14+ (candle/light) on PATH, via WIX, or downloaded under tools\wix3.

$ErrorActionPreference = "Stop"

$root = Resolve-Path (Join-Path $PSScriptRoot "..")
Set-Location $root

function Find-WixBinPath {
    if ($env:WIX) {
        $candidate = Join-Path $env:WIX "bin"
        if (Test-Path (Join-Path $candidate "candle.exe")) { return $candidate }
    }

    $local = Join-Path $root "tools\wix3"
    if (Test-Path (Join-Path $local "candle.exe")) { return $local }

    $cmd = Get-Command candle.exe -ErrorAction SilentlyContinue
    if ($cmd) { return (Split-Path $cmd.Source -Parent) }

    return $null
}

function Ensure-WixBinaries {
    $existing = Find-WixBinPath
    if ($existing) { return $existing }

    Write-Host "WiX 3 not found; downloading binaries to tools\wix3 ..."
    $tools = Join-Path $root "tools\wix3"
    New-Item -ItemType Directory -Force -Path $tools | Out-Null
    $zip = Join-Path $env:TEMP "wix314-binaries.zip"
    Invoke-WebRequest `
        -Uri "https://github.com/wixtoolset/wix3/releases/download/wix3141rtm/wix314-binaries.zip" `
        -OutFile $zip
    Expand-Archive -Path $zip -DestinationPath $tools -Force
    Remove-Item $zip -Force -ErrorAction SilentlyContinue

    if (-not (Test-Path (Join-Path $tools "candle.exe"))) {
        throw "Failed to download WiX 3 binaries into tools\wix3"
    }
    return $tools
}

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    throw "cargo not found on PATH"
}

$wixCheck = cargo wix --version 2>&1
if ($LASTEXITCODE -ne 0) {
    throw "cargo-wix not found. Install with: cargo install cargo-wix"
}

$wixBin = Ensure-WixBinaries
Write-Host "Using WiX tools in: $wixBin"

Write-Host "Building release binary..."
cargo build --release
if ($LASTEXITCODE -ne 0) { throw "cargo build --release failed" }

Write-Host "Building MSI..."
cargo wix --no-build --nocapture --bin-path $wixBin --name Blink
if ($LASTEXITCODE -ne 0) { throw "cargo wix failed" }

$meta = cargo metadata --format-version 1 --no-deps | ConvertFrom-Json
$wixOut = Join-Path $meta.target_directory "wix"
$msi = Get-ChildItem (Join-Path $wixOut "*.msi") -ErrorAction SilentlyContinue |
    Sort-Object LastWriteTime -Descending |
    Select-Object -First 1
if (-not $msi) { throw "No MSI found under $wixOut" }

Write-Host ""
Write-Host "Installer: $($msi.FullName)"
