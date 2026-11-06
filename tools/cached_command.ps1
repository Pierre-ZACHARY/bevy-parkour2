#!/usr/bin/env pwsh
param([Parameter(ValueFromRemainingArguments=$true)][string[]]$CommandArgs)
$ErrorActionPreference = 'Stop'
& (Join-Path $PSScriptRoot 'setup_cache.ps1')
$root = Split-Path $PSScriptRoot -Parent
$frontend = Join-Path $root ($IsWindows ? '.compiler-cache/bin/sccache.exe' : '.compiler-cache/bin/sccache')
& $frontend --run @CommandArgs
exit $LASTEXITCODE
