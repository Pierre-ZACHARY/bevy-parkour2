[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$revision = 'd62e9f213321befce6f870971e191c9f3946b967'
$archiveHash = 'e6ada78542ad55bf1891f36d48ecd7d19fa3b7d797b553e666a775c13d373aa0'
$tools = Join-Path $PSScriptRoot '.tools'
$source = Join-Path $tools "pulumi-rust-$revision"
$bin = Join-Path $tools 'bin'
New-Item -ItemType Directory -Force $tools, $bin | Out-Null
if (-not (Test-Path (Join-Path $source 'sdk\rust\pulumi\Cargo.toml'))) {
    $archive = Join-Path $tools 'pulumi-rust.zip'
    & curl.exe --fail --location --silent --show-error "https://codeload.github.com/pulumi-labs/pulumi-rust/zip/$revision" -o $archive
    if ($LASTEXITCODE -ne 0) { throw 'Cannot download Pulumi Rust sources' }
    if ((Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant() -ne $archiveHash) {
        throw 'Pulumi Rust archive checksum mismatch'
    }
    Expand-Archive -LiteralPath $archive -DestinationPath $tools -Force
}
$oldToolchain = $env:GOTOOLCHAIN
Push-Location (Join-Path $source 'pulumi-language-rust')
try {
    $env:GOTOOLCHAIN = 'auto'
    & go build -o (Join-Path $bin 'pulumi-language-rust.exe') .
    if ($LASTEXITCODE -ne 0) { throw 'Cannot build the Pulumi Rust language plugin' }
} finally {
    $env:GOTOOLCHAIN = $oldToolchain
    Pop-Location
}
& cargo build --locked --manifest-path (Join-Path $PSScriptRoot 'Cargo.toml')
if ($LASTEXITCODE -ne 0) { throw 'Cannot build the infrastructure program' }
Write-Output 'Pulumi Rust ready. Use infra/pulumi.ps1 preview.'
