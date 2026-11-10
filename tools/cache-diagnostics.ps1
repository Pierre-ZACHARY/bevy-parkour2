#!/usr/bin/env pwsh
# Raw sccache debug output stays private to the runner. Export only keys/results.
param([switch]$Start)
$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
$raw = Join-Path $root '.compiler-cache/diagnostics-raw.log'
$output = Join-Path $root '.compiler-cache/diagnostics'
if ($Start) {
    New-Item -ItemType Directory -Force $output | Out-Null
    @(
        'SCCACHE_LOG=sccache::compiler::compiler=debug,sccache::server=debug'
        "SCCACHE_ERROR_LOG=$raw"
        "PROJECT_CACHE_DIAGNOSTICS=$output"
    ) | Out-File -FilePath $env:GITHUB_ENV -Append -Encoding utf8
    exit 0
}
# Flush this job's daemon before filtering. Never upload the raw debug log.
& (Join-Path $root '.compiler-cache/bin/sccache') --stop-server
if (Test-Path $raw) {
    Get-Content $raw | ForEach-Object {
        if ($_ -match '\[([a-zA-Z0-9_.-]+)\]: (Hash key: [a-f0-9]+|compile result: (cache hit|cache miss|not cached|not cacheable|compile failed|cache error))$') {
            $Matches[0]
        }
    } | Set-Content (Join-Path $output 'sccache.log')
}
# Generated sources/native libraries can invalidate downstream Rust keys even
# when arguments match. Keep names and SHA256 only, never build-script output.
Get-ChildItem (Join-Path $root 'target') -Recurse -File -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -match '[\\/]build[\\/]' -and ($_.Extension -in '.rs','.lib','.a' -or $_.Name -eq 'output') } |
    Sort-Object FullName | ForEach-Object {
        $relative = [IO.Path]::GetRelativePath($root, $_.FullName)
        "$relative $((Get-FileHash -LiteralPath $_.FullName -Algorithm SHA256).Hash)"
    } | Set-Content (Join-Path $output 'generated-inputs.txt')
