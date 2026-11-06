# Publish only managed public Sentry client configuration, never provider tokens.
[CmdletBinding()]
param([string] $Stack = 'Pierre-ZACHARY/bevy-parkour2-infra/dev')
$ErrorActionPreference='Stop'
$config=Get-Content (Join-Path $PSScriptRoot '../tools/infisical-config.json') -Raw | ConvertFrom-Json
try {
    if ($env:GITHUB_ACTIONS -eq 'true') {
        if ($env:GITHUB_EVENT_NAME -ne 'workflow_dispatch' -or $env:GITHUB_REF -ne 'refs/heads/main' -or -not $env:INFISICAL_INFRA_IDENTITY_ID) { throw 'Restricted infrastructure identity required' }
        $url=$env:ACTIONS_ID_TOKEN_REQUEST_URL+'&audience='+[Uri]::EscapeDataString($config.domain)
        $oidc=Invoke-RestMethod $url -Headers @{Authorization="Bearer $env:ACTIONS_ID_TOKEN_REQUEST_TOKEN"}
        $body=@{identityId=$env:INFISICAL_INFRA_IDENTITY_ID;jwt=$oidc.value}|ConvertTo-Json -Compress
        $auth=Invoke-RestMethod ($config.domain+'/api/v1/auth/oidc-auth/login') -Method Post -ContentType application/json -Body $body
        $token=$auth.accessToken
    } elseif ($env:INFISICAL_TOKEN) { $token=$env:INFISICAL_TOKEN }
    else {
        $token=(& infisical user get token --plain --silent --domain $config.domain 2>$null | Out-String).Trim()
        if ($LASTEXITCODE -ne 0 -or -not $token) { throw 'No Infisical login' }
    }
    $headers=@{Authorization="Bearer $token"}
    $query='?projectId='+$config.buildProjectId+'&environment='+$config.environment+'&secretPath='+[Uri]::EscapeDataString($config.appPath)+'&includeImports=false&includePersonalOverrides=false'
    $current=Invoke-RestMethod ($config.domain+'/api/v4/secrets'+$query) -Headers $headers
    $existing=@{}
    foreach($s in $current.secrets){$existing[$s.secretKey]=$s.secretValue}
    $updated=0
    foreach($pair in @(@('SENTRY_DSN','sentryDsnPublic'),@('SENTRY_ORG','sentryOrganization'),@('SENTRY_PROJECT','sentryProject'))) {
        $raw=& pulumi --cwd $PSScriptRoot stack output $pair[1] --stack $Stack --json --show-secrets --non-interactive 2>$null
        if ($LASTEXITCODE -ne 0) { throw 'Cannot read managed client output' }
        $value=$raw | ConvertFrom-Json
        if ($value -isnot [string] -or -not $value -or $value -match '[\x00-\x1f\x7f]') { throw 'Invalid managed output' }
        if ($existing[$pair[0]] -ceq $value) { continue }
        $body=@{projectId=$config.buildProjectId;environment=$config.environment;secretPath=$config.appPath;secretValue=$value}|ConvertTo-Json -Compress
        $method=if($existing.ContainsKey($pair[0])){'Patch'}else{'Post'}
        $null=Invoke-RestMethod ($config.domain+'/api/v4/secrets/'+$pair[0]) -Headers $headers -Method $method -ContentType application/json -Body $body
        $updated++
    }

} catch { throw 'Cannot sync managed Sentry client configuration to Infisical. Check stack output access and build-project write permission. Details redacted.' }
Write-Output "Managed build configuration synchronized ($updated changed values)."
