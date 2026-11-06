# Scope credentials, backend and plugin PATH to the Pulumi child process.
# No secret values are written to the console or supplied in command arguments.
[CmdletBinding()]
param([Parameter(ValueFromRemainingArguments = $true)][string[]] $PulumiArgs)
$ErrorActionPreference = 'Stop'
$names = @('OVH_APPLICATION_KEY','OVH_APPLICATION_SECRET','OVH_CONSUMER_KEY','SENTRY_AUTH_TOKEN','SONAR_TOKEN')
$optionalNames = @('SENTRY_RELEASE_TOKEN')
$saved = @{}
foreach ($name in ($names + $optionalNames + @('PATH','PULUMI_BACKEND_URL','INFISICAL_TOKEN','INFISICAL_AUTH_METHOD'))) {
    $saved[$name] = [Environment]::GetEnvironmentVariable($name, 'Process')
}
try {
    if (-not $PulumiArgs) { $PulumiArgs = @('preview') }
    # Read-only output/config commands use the existing Pulumi login alone.
    $missing = @($names | Where-Object { -not [Environment]::GetEnvironmentVariable($_, 'Process') })
    if ($PulumiArgs[0] -in @('preview','up','refresh','destroy','import') -and $missing.Count -gt 0) {
        $config = Get-Content (Join-Path $PSScriptRoot '../tools/infisical-config.json') -Raw | ConvertFrom-Json
        $raw = & infisical sm secrets export --silent --domain $config.domain --projectId $config.infraProjectId --env $config.environment --path / --format json --include-imports=false --secret-overriding=false --expand=false 2>$null
        if ($LASTEXITCODE -ne 0) { throw 'Infisical infrastructure access denied. Run infisical login --domain https://eu.infisical.com and check project membership.' }
        foreach ($secret in ($raw | ConvertFrom-Json)) {
            if (($names + $optionalNames) -contains $secret.key) { [Environment]::SetEnvironmentVariable($secret.key, $secret.value, 'Process') }
        }
        foreach ($name in $names) {
            if (-not [Environment]::GetEnvironmentVariable($name, 'Process')) { throw "Missing infrastructure credential $name in Infisical" }
        }
    }
    if ($PulumiArgs[0] -in @('preview','up','refresh','destroy','import')) {
        . (Join-Path $PSScriptRoot 'infisical-auth.ps1')
        $env:INFISICAL_TOKEN = Get-InfraInfisicalToken -Domain 'https://eu.infisical.com'
        $env:INFISICAL_AUTH_METHOD = 'token'
    }
    $env:PULUMI_BACKEND_URL = 'https://api.pulumi.com'
    $env:PATH = (Join-Path $PSScriptRoot '.tools\bin') + [IO.Path]::PathSeparator + $env:PATH
    if (-not $PulumiArgs) { $PulumiArgs = @('preview') }
    & pulumi --cwd $PSScriptRoot @PulumiArgs
    if ($LASTEXITCODE -ne 0) { throw "Pulumi exited with code $LASTEXITCODE" }
    if ($PulumiArgs[0] -eq 'up' -and $PulumiArgs -notcontains '--preview-only') {
        $stack = ''
        for ($i=0; $i -lt $PulumiArgs.Count; $i++) {
            if ($PulumiArgs[$i] -in @('--stack','-s')) { $stack = $PulumiArgs[$i+1] }
            elseif ($PulumiArgs[$i] -match '^--stack=(.+)$') { $stack = $Matches[1] }
        }
        & (Join-Path $PSScriptRoot 'export-cache-config.ps1') -Stack $stack
        if ($stack) { & (Join-Path $PSScriptRoot 'sync-build-config.ps1') -Stack $stack }
        else { & (Join-Path $PSScriptRoot 'sync-build-config.ps1') }
    }
} finally {
    foreach ($name in $saved.Keys) { [Environment]::SetEnvironmentVariable($name, $saved[$name], 'Process') }
}
