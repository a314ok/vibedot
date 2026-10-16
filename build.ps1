# Vibedot build script for Windows (PowerShell).
#
#   .\build.ps1                     build the editor (release) and open it
#   .\build.ps1 editor [project]
#   .\build.ps1 run [project]       run a game in the player (default: Neon Drift)
#   .\build.ps1 release             build editor + player in release mode
#   .\build.ps1 test                fmt check + clippy -D warnings + all tests
#   .\build.ps1 export <project>    package a game as dist\windows\<Game>\<Game>.exe + .zip
#   .\build.ps1 sdk                 package the editor as dist\sdk-windows\Vibedot + .zip
#   .\build.ps1 clean
param(
    [Parameter(Position = 0)][string]$Command = "editor",
    [Parameter(Position = 1)][string]$Project = ""
)
$ErrorActionPreference = "Stop"
$Root = $PSScriptRoot
$Ws = Join-Path $Root "vibedot"
$Dist = Join-Path $Root "dist"

function Say($m) { Write-Host "♦ $m" -ForegroundColor Magenta }
function Info($m) { Write-Host "  $m" -ForegroundColor Cyan }
function Cargo([string[]]$CargoArgs) {
    Push-Location $Ws
    try {
        & cargo @CargoArgs
        if ($LASTEXITCODE -ne 0) { throw "cargo $($CargoArgs -join ' ') failed" }
    } finally { Pop-Location }
}
function Require-Project($p) {
    if (-not (Test-Path (Join-Path $p "project.vibedot"))) { throw "$p is not a Vibedot project (no project.vibedot)" }
}

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) { throw "cargo not found - install Rust from https://rustup.rs" }

switch ($Command) {
    "editor" {
        Cargo @("build", "--release", "-p", "vd_editor", "-p", "vd_player")
        Say "Opening the editor"
        $a = @(); if ($Project) { $a += $Project }
        & (Join-Path $Ws "target\release\vibedot.exe") @a
    }
    "run" {
        if (-not $Project) { $Project = Join-Path $Ws "examples\neon_drift" }
        Require-Project $Project
        Cargo @("build", "--release", "-p", "vd_player")
        & (Join-Path $Ws "target\release\vibedot-player.exe") $Project
    }
    { $_ -in "release", "build" } {
        Say "Building editor + player (release)"
        Cargo @("build", "--release", "-p", "vd_editor", "-p", "vd_player")
        Info (Join-Path $Ws "target\release\vibedot.exe")
    }
    { $_ -in "test", "check" } {
        Say "rustfmt"; Cargo @("fmt", "--all", "--check")
        Say "clippy";  Cargo @("clippy", "--workspace", "--all-targets", "--", "-D", "warnings")
        Say "tests";   Cargo @("test", "--workspace")
        Say "All checks passed"
    }
    { $_ -in "export", "package" } {
        if (-not $Project) { throw "usage: .\build.ps1 export <project>" }
        Require-Project $Project
        $Project = (Resolve-Path $Project).Path
        Say "Building the player (dist profile: fat LTO)"
        Cargo @("build", "--release", "-p", "vd_editor")
        Cargo @("build", "--profile", "dist", "-p", "vd_player")
        $player = Join-Path $Ws "target\dist\vibedot-player.exe"
        $out = Join-Path $Dist "windows"
        New-Item -ItemType Directory -Force $out | Out-Null
        Say "Exporting $(Split-Path $Project -Leaf)"
        $log = & (Join-Path $Ws "target\release\vibedot.exe") export $Project --out $out --platform windows --player $player
        if ($LASTEXITCODE -ne 0) { throw "export failed" }
        $log | ForEach-Object { Info $_ }
        $app = ($log | Select-String '^Exported (.*) \(\d+ KiB pack\)$').Matches[0].Groups[1].Value
        $zip = Join-Path $out (((Split-Path $app -Leaf).ToLower() -replace ' ', '-') + "-windows.zip")
        Compress-Archive -Force -Path $app -DestinationPath $zip
        Info "→ $zip"
        Say "Done"
    }
    "sdk" {
        Say "Building Vibedot editor + player (dist profile)"
        Cargo @("build", "--profile", "dist", "-p", "vd_editor", "-p", "vd_player")
        $dest = Join-Path $Dist "sdk-windows\Vibedot"
        if (Test-Path $dest) { Remove-Item -Recurse -Force $dest }
        New-Item -ItemType Directory -Force $dest | Out-Null
        Copy-Item (Join-Path $Ws "target\dist\vibedot.exe"), (Join-Path $Ws "target\dist\vibedot-player.exe") $dest
        Copy-Item -Recurse (Join-Path $Ws "examples") (Join-Path $dest "examples")
        $zip = Join-Path $Dist "sdk-windows\vibedot-windows.zip"
        Compress-Archive -Force -Path $dest -DestinationPath $zip
        Info "→ $dest"; Info "→ $zip"
        Say "Done"
    }
    "clean" {
        Cargo @("clean")
        if (Test-Path $Dist) { Remove-Item -Recurse -Force $Dist }
    }
    default { throw "unknown command '$Command' (see the header of build.ps1)" }
}
