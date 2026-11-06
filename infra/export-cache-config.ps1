# Export only the public compilerCache stack output. No --show-secrets.
[CmdletBinding()]
param([string] $Stack)
$ErrorActionPreference = 'Stop'
$allowed = @('SCCACHE_BUCKET','SCCACHE_REGION','SCCACHE_ENDPOINT','SCCACHE_S3_USE_SSL','SCCACHE_S3_ENABLE_VIRTUAL_HOST_STYLE','SCCACHE_S3_KEY_PREFIX','SCCACHE_S3_RW_MODE','AWS_EC2_METADATA_DISABLED')
$argsList = @('--cwd', $PSScriptRoot, 'stack', 'output', 'compilerCache', '--json', '--non-interactive')
if ($Stack) { $argsList += @('--stack', $Stack) }
$savedBackend = $env:PULUMI_BACKEND_URL
try {
    $env:PULUMI_BACKEND_URL = 'https://api.pulumi.com'
    $raw = & pulumi @argsList
    if ($LASTEXITCODE -ne 0) { throw 'Cannot read public compilerCache output; run infra/pulumi.ps1 up first' }
} finally { $env:PULUMI_BACKEND_URL = $savedBackend }
$config = ($raw -join "`n") | ConvertFrom-Json
if (@($config.PSObject.Properties).Count -ne $allowed.Count) { throw 'Unexpected compilerCache output schema' }
$lines = @('# Public settings exported from Pulumi; refresh with infra/export-cache-config.ps1.')
foreach ($name in $allowed) {
    $property = $config.PSObject.Properties[$name]
    if (-not $property -or $property.Value -isnot [string] -or $property.Value -match '[\x00-\x1f\x7f]') { throw "Invalid public cache setting: $name" }
    $lines += "$name=$($property.Value)"
}
$cache = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../.compiler-cache'))
New-Item -ItemType Directory -Force $cache | Out-Null
$file = Join-Path $cache 'public.env'
$content = ($lines -join "`n") + "`n"
if (-not (Test-Path -LiteralPath $file) -or [IO.File]::ReadAllText($file) -ne $content) {
    $temporary = Join-Path $cache ('.public-' + [Guid]::NewGuid().ToString('N') + '.tmp')
    try {
        [IO.File]::WriteAllText($temporary,$content,[Text.UTF8Encoding]::new($false))
        if (Test-Path -LiteralPath $file) { [IO.File]::Replace($temporary,$file,$null) }
        else { [IO.File]::Move($temporary,$file) }
    } finally { if (Test-Path -LiteralPath $temporary) { Remove-Item -LiteralPath $temporary } }
}
Write-Output 'Public Pulumi cache settings exported to .compiler-cache/public.env (ignored by Git).'
