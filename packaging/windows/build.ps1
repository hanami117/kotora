# Packs Kotora for Windows from an existing release build: the installer, the zip and their checksums.
#
#   cargo build --release --locked -p kotora -p kotora-ctl
#   pwsh packaging\windows\build.ps1 -Version 0.2.1 -Build 1
#
# Out (in -OutDir, default target\windows-release), <release> = <version>, or <version>-<build> from build 2 on:
#   Kotora-<release>-windows-x64-setup.exe (+ .sha256)   the installer (packaging\windows\kotora.iss)
#   Kotora-windows-x64-setup.exe (+ .sha256)             the same bytes under a stable name (the site's button,
#                                                        latest.json: releases/latest/download/…)
#   Kotora-<release>-windows-x64.zip (+ .sha256)         the same programs without an installer
# and -StageDir (default target\windows-dist): the files that go into both.
#
# Nothing is signed. The hook for SignPath (or signtool) is Sign-Files below.
param(
    [Parameter(Mandatory = $true)][string]$Version,
    [int]$Build = 1,
    [string]$BinDir = "target\release",
    [string]$StageDir = "target\windows-dist",
    [string]$OutDir = "target\windows-release"
)
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$root = (Resolve-Path (Join-Path $PSScriptRoot "..\..")).Path
Set-Location $root
if ($Version -notmatch '^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$') { throw "bad version: $Version" }
if ($Build -lt 1) { throw "bad build: $Build" }
$release = if ($Build -eq 1) { $Version } else { "$Version-$Build" }
# Four numbers for the installer's own version details: the version without a pre-release tag, then the build.
$fileVersion = "$(($Version -split '-')[0]).$Build"

# SIGNING HOOK. Today: nothing. With SignPath, the programs are signed here, before they are packed, and
# the installer afterwards (kotora.iss says how Inno signs it and its uninstaller).
function Sign-Files([string[]]$Files) {
    foreach ($f in $Files) { Write-Host "not signed: $f" }
}

function Write-Sha256([string]$File) {
    $name = Split-Path $File -Leaf
    $hash = (Get-FileHash -Algorithm SHA256 $File).Hash.ToLowerInvariant()
    # The sha256sum format: hash, two spaces, name, one line feed.
    [System.IO.File]::WriteAllText("$File.sha256", "$hash  $name`n", (New-Object System.Text.ASCIIEncoding))
    Write-Host "$hash  $name"
}

# --- the files -----------------------------------------------------------------------------------
foreach ($d in $StageDir, $OutDir) {
    if (Test-Path $d) { Remove-Item -Recurse -Force $d }
    New-Item -ItemType Directory -Force $d | Out-Null
}
$stage = (Resolve-Path $StageDir).Path
$out = (Resolve-Path $OutDir).Path
Copy-Item (Join-Path $BinDir "kotora.exe"), (Join-Path $BinDir "kot.exe"), "packaging\windows\README-WINDOWS.txt" $stage
Copy-Item "LICENSE" (Join-Path $stage "LICENSE.txt")
Copy-Item "THIRD_PARTY_NOTICES.md" (Join-Path $stage "THIRD_PARTY_NOTICES.txt")
Sign-Files (Join-Path $stage "kotora.exe"), (Join-Path $stage "kot.exe")

# The programs carry the version they were built from: never pack a stale build under a new name.
$said = (& (Join-Path $stage "kot.exe") --version | Out-String).Trim()
if ($said -notmatch [regex]::Escape("$Version (build $Build)")) { throw "kot.exe says '$said', expected $Version (build $Build)" }

# --- the installer -------------------------------------------------------------------------------
$iscc = (Get-Command iscc.exe -ErrorAction SilentlyContinue).Source
if (-not $iscc) {
    $iscc = @("${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe", "$env:ProgramFiles\Inno Setup 6\ISCC.exe", "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe") |
        Where-Object { Test-Path $_ } | Select-Object -First 1
}
if (-not $iscc) { throw "Inno Setup 6 (ISCC.exe) not found: install it from https://jrsoftware.org/isdl.php" }
& $iscc /Qp "/DAppVersion=$Version" "/DFileVersion=$fileVersion" "/DRelease=$release" "/DSourceDir=$stage" "/DOutputDir=$out" "packaging\windows\kotora.iss"
if ($LASTEXITCODE -ne 0) { throw "iscc failed ($LASTEXITCODE)" }
$setup = Join-Path $out "Kotora-$release-windows-x64-setup.exe"
if (-not (Test-Path $setup)) { throw "no installer at $setup" }
Sign-Files $setup
$stable = Join-Path $out "Kotora-windows-x64-setup.exe"
Copy-Item $setup $stable

# --- the zip -------------------------------------------------------------------------------------
$zip = Join-Path $out "Kotora-$release-windows-x64.zip"
Compress-Archive -Path (Join-Path $stage "*") -DestinationPath $zip -CompressionLevel Optimal

foreach ($f in $setup, $stable, $zip) { Write-Sha256 $f }
Get-ChildItem $out | Format-Table Name, Length | Out-Host
