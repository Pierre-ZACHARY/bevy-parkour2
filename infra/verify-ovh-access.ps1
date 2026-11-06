# Exercise the real OVH API from the runner, including denied capabilities.
# No OAuth token, API response body or credential is written to disk/logs.
[CmdletBinding()]
param([string] $Stack = 'Pierre-ZACHARY/bevy-parkour2-infra/dev')
$ErrorActionPreference = 'Stop'
if (-not $env:OVH_CLIENT_ID -or -not $env:OVH_CLIENT_SECRET) {
    throw 'Load the infrastructure Infisical scope before verifying OVH access.'
}
$bucketId = (& pulumi --cwd $PSScriptRoot stack output bucketId --stack $Stack --non-interactive | Out-String).Trim()
if ($LASTEXITCODE -ne 0) { throw 'Cannot read the public bucket ID from Pulumi Cloud.' }
$parts = $bucketId.Split('/')
if ($parts.Count -ne 3 -or @($parts | Where-Object { -not $_ }).Count) { throw 'Invalid public bucket ID.' }
$project, $region, $bucket = @($parts | ForEach-Object { [Uri]::EscapeDataString($_) })
try {
    $token = Invoke-RestMethod 'https://www.ovh.com/auth/oauth2/token' -Method Post -ContentType application/x-www-form-urlencoded -Body @{
        grant_type='client_credentials'; client_id=$env:OVH_CLIENT_ID; client_secret=$env:OVH_CLIENT_SECRET; scope='all'
    }
} catch { throw 'OVH OAuth exchange failed (details redacted).' }
$headers = @{Authorization='Bearer '+$token.access_token}
$checks = @(
    @{url='https://eu.api.ovh.com/1.0/auth/details'; status=200; capability='authentication'},
    @{url="https://eu.api.ovh.com/1.0/cloud/project/$project/region/$region/storage/$bucket"; status=200; capability='existing bucket metadata'},
    @{url='https://eu.api.ovh.com/1.0/me'; status=403; capability='account administration'},
    @{url="https://eu.api.ovh.com/1.0/cloud/project/$project/instance"; status=403; capability='compute'},
    @{url='https://eu.api.ovh.com/v2/iam/policy'; status=403; capability='IAM administration'}
)
foreach ($check in $checks) {
    try { $response = Invoke-WebRequest $check.url -Headers $headers -SkipHttpErrorCheck }
    catch { throw "OVH capability check failed: $($check.capability) (details redacted)." }
    if ([int]$response.StatusCode -ne $check.status) {
        throw "Unexpected HTTP $([int]$response.StatusCode) for $($check.capability); expected $($check.status)."
    }
    Write-Output "Verified OVH $($check.capability): HTTP $($check.status)."
}
