#!/usr/bin/env pwsh
# Only the bootstrap is PowerShell; cache policy and installation live in Rust.
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$cache = Join-Path $root '.compiler-cache'
New-Item -ItemType Directory -Force $cache | Out-Null
$bootstrap = Join-Path $cache ($IsWindows ? 'setup.exe' : 'setup')
$source = Join-Path $PSScriptRoot 'compiler-cache.rs'
$pins = Join-Path $PSScriptRoot 'compiler-cache-pins.txt'
$configSource = Join-Path $PSScriptRoot 'cache-config.rs'
$secretSource = Join-Path $PSScriptRoot 'cache-infisical.rs'
$secretConfig = Join-Path $PSScriptRoot 'infisical-config.json'
if (-not (Test-Path $bootstrap) -or @($source,$pins,$configSource,$secretSource,$secretConfig | Where-Object { (Get-Item $_).LastWriteTimeUtc -gt (Get-Item $bootstrap).LastWriteTimeUtc }).Count -gt 0) {
    & rustc --edition=2024 -C opt-level=2 $source -o $bootstrap
    if ($LASTEXITCODE -ne 0) { throw 'Cannot compile the dependency-free cache bootstrap' }
}
& $bootstrap --setup
if ($LASTEXITCODE -ne 0) { throw 'Cannot initialize the project compiler cache' }
