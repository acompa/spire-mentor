# Build the native SimAdvisor (GH #1426): one Rust binary plus its data,
# staged into sts2-pycli/dist/SimAdvisor/ -- the directory package.ps1 ships
# as mods/SpireMentor/SimAdvisor/. It replaced the PyInstaller bundle of the
# Python advisor (removed after the native one shipped).
#
# Gates (any failure aborts):
#   1. the release build of sts2-rs/crates/sim-advisor
#   2. stage_advisor.py: only the declared champions, hash-checked; the
#      Instinct weights (or SIMADVISOR_NO_INSTINCT=1); gzipped localization
#   3. SimAdvisor.exe --smoke exits 0 (localization, every champion through a
#      real decision, Instinct through 20 decisions, a socket bind)
#
# Needs the Rust toolchain (BUILDING-WINDOWS.md) and uv; no Python ships.
$ErrorActionPreference = "Stop"
$pycli = Split-Path -Parent $PSScriptRoot
$rs = Join-Path (Split-Path -Parent $pycli) "sts2-rs"

Push-Location $rs
try {
    cargo build --release -p sim-advisor --bin SimAdvisor
    if ($LASTEXITCODE -ne 0) { throw "cargo build of SimAdvisor failed" }
} finally { Pop-Location }
# Windows is the shipped build; elsewhere this runs as a check of the script.
$exeName = if ($IsWindows -or $env:OS -eq "Windows_NT") { "SimAdvisor.exe" } else { "SimAdvisor" }
$exe = Join-Path $rs "target/release/$exeName"
if (-not (Test-Path $exe)) { throw "missing $exe" }

Push-Location $pycli
try {
    # Stages the binary under its shipped name (SimAdvisor.exe on Windows,
    # SimAdvisor-<os>-<arch> elsewhere) and runs its --smoke.
    uv run --no-project python scripts/stage_advisor.py $exe --out dist/SimAdvisor --smoke
    if ($LASTEXITCODE -ne 0) { throw "staging the advisor or its --smoke failed" }
} finally { Pop-Location }

$dist = Join-Path $pycli "dist/SimAdvisor"
$sizeMB = [math]::Round((Get-ChildItem $dist -Recurse | Measure-Object Length -Sum).Sum / 1MB)
Write-Host "OK: dist/SimAdvisor ($sizeMB MB, native)"
