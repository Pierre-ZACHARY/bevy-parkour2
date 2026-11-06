# Shared process-only authentication for Pulumi and output synchronization.
function Get-InfraInfisicalToken {
    param([Parameter(Mandatory)][string] $Domain)
    if ($env:GITHUB_ACTIONS -eq 'true') {
        if ($env:GITHUB_EVENT_NAME -ne 'workflow_dispatch' -or $env:GITHUB_REF -ne 'refs/heads/main' -or -not $env:INFISICAL_INFRA_IDENTITY_ID) { throw 'Restricted infrastructure OIDC identity required' }
        $url=$env:ACTIONS_ID_TOKEN_REQUEST_URL+'&audience='+[Uri]::EscapeDataString($Domain)
        $oidc=Invoke-RestMethod $url -Headers @{Authorization="Bearer $env:ACTIONS_ID_TOKEN_REQUEST_TOKEN"}
        $body=@{identityId=$env:INFISICAL_INFRA_IDENTITY_ID;jwt=$oidc.value}|ConvertTo-Json -Compress
        $auth=Invoke-RestMethod ($Domain+'/api/v1/auth/oidc-auth/login') -Method Post -ContentType application/json -Body $body
        return $auth.accessToken
    }
    if ($env:INFISICAL_TOKEN) { return $env:INFISICAL_TOKEN }
    $token=(& infisical user get token --plain --silent --domain $Domain 2>$null | Out-String).Trim()
    if ($LASTEXITCODE -ne 0 -or -not $token) { throw 'Infisical login required' }
    return $token
}
