# Installs, checks and uninstalls Kotora with the installer build.ps1 made. For a CI runner or a throwaway
# Windows account: it installs for the current user and writes (then removes) marker files in the real
# %APPDATA%\kotora and %LOCALAPPDATA%\kotora, to prove that an uninstall keeps the user's data.
# Never run it on a PC where Kotora is in use.
#
#   pwsh packaging\windows\smoke.ps1 -Setup target\windows-release\Kotora-windows-x64-setup.exe -Version 0.2.1
param(
    [Parameter(Mandatory = $true)][string]$Setup,
    [Parameter(Mandatory = $true)][string]$Version
)
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$setupPath = (Resolve-Path $Setup).Path
$dir = Join-Path ([System.IO.Path]::GetTempPath()) "Kotora Smoke $PID"   # a space in the path, on purpose
$log = Join-Path ([System.IO.Path]::GetTempPath()) "kotora-smoke-$PID"
New-Item -ItemType Directory -Force $log | Out-Null
$startMenu = Join-Path ([Environment]::GetFolderPath("Programs")) "Kotora.lnk"
$desktop = Join-Path ([Environment]::GetFolderPath("DesktopDirectory")) "Kotora.lnk"
$data = @((Join-Path $env:APPDATA "kotora"), (Join-Path $env:LOCALAPPDATA "kotora"))
$ok = $false
$uninstallKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{17091F1F-7829-4B92-964B-D2AA5E246767}_is1"

function Check([bool]$ok, [string]$what) {
    if ($ok) { Write-Host "ok    $what" } else { throw "FAILED: $what" }
}
function UserPath { [string](Get-ItemProperty -Path "HKCU:\Environment" -Name Path -ErrorAction SilentlyContinue | ForEach-Object { $_.Path }) }
function OnPath { (UserPath) -split ';' | Where-Object { $_ -ieq $dir } | Measure-Object | ForEach-Object { $_.Count -gt 0 } }
function Run([string]$exe, [string[]]$arguments, [string]$what) {
    $p = Start-Process -FilePath $exe -ArgumentList $arguments -Wait -PassThru
    Check ($p.ExitCode -eq 0) "$what (exit code $($p.ExitCode))"
}
function Install([string]$tasks, [string]$name) {
    Run $setupPath @("/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/DIR=`"$dir`"", "/TASKS=`"$tasks`"", "/LOG=`"$log\$name.log`"") "install ($name)"
}
function Uninstall([string[]]$extra, [string]$name) {
    $un = Join-Path $dir "unins000.exe"
    Check (Test-Path $un) "the uninstaller is there"
    Run $un (@("/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/LOG=`"$log\$name.log`"") + $extra) "uninstall ($name)"
    # The uninstaller finishes from a copy of itself: give it a moment to delete the folder.
    foreach ($i in 1..60) { if (-not (Test-Path $dir)) { break }; Start-Sleep -Milliseconds 500 }
}

try {
    Check (-not (Test-Path $uninstallKey)) "Kotora is not installed yet on this machine"
    $pathBefore = UserPath

    # --- 1. a plain install: no admin, the PATH task on (the default), no desktop shortcut ------------
    Install "addtopath" "install-1"
    foreach ($f in "kotora.exe", "kot.exe", "unins000.exe", "README-WINDOWS.txt", "LICENSE.txt", "THIRD_PARTY_NOTICES.txt") {
        Check (Test-Path (Join-Path $dir $f)) "$f is installed"
    }
    $said = (& (Join-Path $dir "kot.exe") --version | Out-String).Trim()
    Write-Host "      $said"
    Check ($said -match [regex]::Escape($Version)) "kot --version says $Version"
    $said = (& (Join-Path $dir "kotora.exe") --version | Out-String).Trim()
    Check ($said -match [regex]::Escape($Version)) "kotora --version says $Version ($said)"
    Check (Test-Path $startMenu) "the Start menu shortcut is there"
    Check (-not (Test-Path $desktop)) "no desktop shortcut unless asked"
    Check (OnPath) "the folder is on the user's PATH"
    $un = Get-ItemProperty $uninstallKey
    Check ($un.DisplayName -eq "Kotora") "listed in Installed apps as '$($un.DisplayName)'"
    Check ($un.DisplayVersion -eq $Version) "listed with version $($un.DisplayVersion)"
    Check (-not (Test-Path "HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\{17091F1F-7829-4B92-964B-D2AA5E246767}_is1")) "nothing was written for all users"
    # kot from the PATH, as a new terminal would find it.
    $env:Path = "$env:Path;$dir"
    Check ((& kot --version | Out-String) -match [regex]::Escape($Version)) "kot runs from the PATH"
    # With no app running kot must say so (and so must the installer's own 'kot quit').
    $said = & (Join-Path $dir "kot.exe") status 2>&1 | Out-String
    Check ($LASTEXITCODE -ne 0 -and $said -match "running") "kot status without the app: $($said.Trim())"

    # The user's data and what the app writes for the system (stand-ins: the app itself never ran here).
    foreach ($d in $data) { New-Item -ItemType Directory -Force $d | Out-Null; Set-Content (Join-Path $d "smoke-marker.txt") "keep me" }
    $run = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
    New-Item -Path $run -Force | Out-Null
    Set-ItemProperty -Path $run -Name "Kotora" -Value "`"$dir\kotora.exe`" --hidden"
    $appId = "HKCU:\Software\Classes\AppUserModelId\io.github.hanami117.kotora"
    New-Item -Path $appId -Force | Out-Null
    Set-ItemProperty -Path $appId -Name "DisplayName" -Value "Kotora"

    # --- 2. an update over it: same folder without /DIR, the desktop shortcut this time, PATH once ----
    Run $setupPath @("/VERYSILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/TASKS=`"addtopath,desktopicon`"", "/LOG=`"$log\install-2.log`"") "install again (an update)"
    Check (Test-Path (Join-Path $dir "kotora.exe")) "the update went to the same folder"
    Check (Test-Path $desktop) "the desktop shortcut is there when asked"
    Check (((UserPath) -split ';' | Where-Object { $_ -ieq $dir } | Measure-Object).Count -eq 1) "the folder is on the PATH once"

    # --- 3. uninstall: the programs go, the user's data stays ----------------------------------------
    Uninstall @() "uninstall-1"
    Check (-not (Test-Path $dir)) "the install folder is gone"
    Check (-not (Test-Path $startMenu)) "the Start menu shortcut is gone"
    Check (-not (Test-Path $desktop)) "the desktop shortcut is gone"
    Check (-not (OnPath)) "the PATH entry is gone"
    $items = { param($p) @($p -split ';' | Where-Object { $_ -ne '' }) -join ';' }
    Check ((& $items (UserPath)) -eq (& $items $pathBefore)) "the user's PATH has what it had before"
    Check (-not (Test-Path $uninstallKey)) "no longer listed in Installed apps"
    Check ($null -eq (Get-ItemProperty -Path $run -Name "Kotora" -ErrorAction SilentlyContinue)) "the start-at-login entry is gone"
    Check (-not (Test-Path $appId)) "the app id's registration is gone"
    foreach ($d in $data) { Check (Test-Path (Join-Path $d "smoke-marker.txt")) "kept: $d" }

    # --- 4. install without the PATH task, uninstall with /REMOVEDATA: the data goes too --------------
    Install "" "install-3"
    Check (-not (OnPath)) "no PATH entry when the task is off"
    Uninstall @("/REMOVEDATA") "uninstall-2"
    Check (-not (Test-Path $dir)) "the install folder is gone again"
    foreach ($d in $data) { Check (-not (Test-Path $d)) "deleted when asked: $d" }
    Write-Host "Install and uninstall: all checks passed."
    $ok = $true
}
finally {
    Write-Host "--- installer logs: $log"
    Get-ChildItem $log -ErrorAction SilentlyContinue | ForEach-Object { Write-Host "--- $($_.Name) (last lines)"; Get-Content $_.FullName -Tail 15 }
    foreach ($d in $data) { Remove-Item (Join-Path $d "smoke-marker.txt") -ErrorAction SilentlyContinue }
}
# `kot status` above failed on purpose and left its exit code behind: say how the checks went.
if ($ok) { exit 0 } else { exit 1 }
