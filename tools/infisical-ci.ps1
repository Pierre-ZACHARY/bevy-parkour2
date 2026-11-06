# GitHub OIDC exchange. Tokens exist only in this process; exported capabilities
# use GitHub's masked, ephemeral job environment, never a repository .env file.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][ValidateSet('cache','app','analysis','publish','infra')][string] $Scope,
    [Parameter(Mandatory)][string] $IdentityId,
    [switch] $VerifyIsolation
)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:GITHUB_EVENT_NAME -in @('pull_request','pull_request_target')) {
    throw 'Infisical CI credentials are reserved for trusted GitHub jobs.'
}
if (-not $IdentityId -or -not $env:ACTIONS_ID_TOKEN_REQUEST_URL -or -not $env:GITHUB_ENV) {
    throw 'Configure the matching Infisical OIDC identity and id-token: write.'
}
$config = Get-Content (Join-Path $PSScriptRoot 'infisical-config.json') -Raw | ConvertFrom-Json
$path = '/'
$optionalNames = @()
switch ($Scope) {
    cache { $project=$config.buildProjectId; $path=$config.cachePath; $names=@('AWS_ACCESS_KEY_ID','AWS_SECRET_ACCESS_KEY') }
    app { $project=$config.buildProjectId; $path=$config.appPath; $names=@('SENTRY_DSN','SENTRY_ORG','SENTRY_PROJECT') }
    analysis { $project=$config.buildProjectId; $path=$config.analysisPath; $names=@('SONAR_TOKEN') }
    publish {
        $project=$env:INFISICAL_RELEASE_PROJECT_ID
        if (-not $project) { $project=$config.releaseProjectId }
        $names=@('SENTRY_AUTH_TOKEN')
        if (-not $project) { throw 'Provision a private release project with a restricted Sentry CI token; set INFISICAL_RELEASE_PROJECT_ID.' }
    }
    infra {
        $project=$config.infraProjectId
        $names=@('OVH_CLIENT_ID','OVH_CLIENT_SECRET','SENTRY_AUTH_TOKEN','SONAR_TOKEN')
        if (-not $env:PULUMI_ACCESS_TOKEN) { $names += 'PULUMI_ACCESS_TOKEN' }
        $optionalNames=@('SENTRY_RELEASE_TOKEN')
    }
}
try {
    $url=$env:ACTIONS_ID_TOKEN_REQUEST_URL+'&audience='+[Uri]::EscapeDataString($config.domain)
    $oidc=Invoke-RestMethod $url -Headers @{Authorization="Bearer $env:ACTIONS_ID_TOKEN_REQUEST_TOKEN"}
    $body=@{identityId=$IdentityId;jwt=$oidc.value}|ConvertTo-Json -Compress
    $auth=Invoke-RestMethod ($config.domain+'/api/v1/auth/oidc-auth/login') -Method Post -ContentType application/json -Body $body
    $headers=@{Authorization="Bearer $($auth.accessToken)"}
    if ($VerifyIsolation) {
        if ($Scope -ne 'cache') { throw 'Isolation proof requires the build cache scope' }
        foreach ($privateProject in @($config.infraProjectId,$config.releaseProjectId)) {
            $probe=$config.domain+'/api/v4/secrets?projectId='+[Uri]::EscapeDataString($privateProject)+'&environment='+$config.environment+'&secretPath=%2F'
            $denial=Invoke-WebRequest $probe -Headers $headers -SkipHttpErrorCheck
            if ([int]$denial.StatusCode -notin @(401,403,404)) { throw 'Build identity has unexpected private-project access' }
        }
        Write-Output 'Verified build isolation: infrastructure and release projects denied.'
    }
    $url=$config.domain+'/api/v4/secrets?projectId='+[Uri]::EscapeDataString($project)+'&environment='+$config.environment+'&secretPath='+[Uri]::EscapeDataString($path)+'&includeImports=false&includePersonalOverrides=false&expandSecretReferences=false'
    $response=Invoke-RestMethod $url -Headers $headers
} catch { throw "Infisical OIDC retrieval failed for $Scope. Check identity claims, membership and required secrets. Details redacted." }
$selected=@{}
foreach($secret in $response.secrets) {
    if (($names + $optionalNames) -contains $secret.secretKey -and -not $secret.secretValueHidden) { $selected[$secret.secretKey]=[string]$secret.secretValue }
}
$names += @($optionalNames | Where-Object { $selected.ContainsKey($_) })
foreach($name in $names) {
    $value=$selected[$name]
    if (-not $value -or $value -match '[\x00-\x1f\x7f]') { throw "Missing or invalid $name in Infisical scope $Scope (redacted)" }
}
foreach($name in $names) {
    $value=$selected[$name]
    # Escape workflow command data before masking, even for single-line values.
    $masked=$value.Replace('%','%25').Replace("`r",'%0D').Replace("`n",'%0A')
    Write-Output "::add-mask::$masked"
    [IO.File]::AppendAllText($env:GITHUB_ENV, "$name=$value`n", [Text.UTF8Encoding]::new($false))
}
if ($Scope -eq 'cache') { [IO.File]::AppendAllText($env:GITHUB_ENV, "SCCACHE_S3_RW_MODE=READ_ONLY`n") }
Write-Output "Loaded Infisical scope $Scope ($($names.Count) variables)."
