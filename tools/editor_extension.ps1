#!/usr/bin/env pwsh
# Builds and installs the project's Jackdaw editor extensions when their
# sources changed since the installed build; otherwise does nothing:
# Flow Graph (crates/bevy_noodle_jackdaw_editor) and the project's own
# (crates/bevy_parkour2_editor). The VS Code "Éditeur (Jackdaw)"
# configuration runs it before opening the editor.
#
# Jackdaw installs a given version once, so each build gets its own:
# `<Cargo.toml version>-dev.<hash of the sources>`. Installing it retires
# the previous one (deleted when the editor next starts). Releases keep
# the plain version: bump it in Cargo.toml and pack by hand (README).

$ErrorActionPreference = 'Stop'
$root = Split-Path $PSScriptRoot -Parent
& (Join-Path $PSScriptRoot 'setup_cache.ps1')

# Each extension and the crates it is built from, itself first.
$extensions = @(
    @{ Label = 'Flow Graph'
       Crates = 'bevy_noodle_jackdaw_editor', 'bevy_noodle_jackdaw', 'bevy_noodle',
                'pose_preview', 'skeleton_retarget' }
    @{ Label = 'bevy-parkour2'
       Crates = 'bevy_parkour2_editor', 'ninja_hair' }
)

# The editor only loads extensions built by its own compiler. The `jd` on
# PATH builds with this repository's toolchain; the one shipped with the
# editor that jackdaw.toml pins builds with the editor's.
$pinned = [regex]::Match((Get-Content -Raw (Join-Path $root 'jackdaw.toml')),
    '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value
$editor = jd editor list | ForEach-Object {
    if ($_ -match "^\s+$([regex]::Escape($pinned))\s.*\s{2,}(\S.*)$") { $Matches[1].Trim() }
} | Select-Object -First 1
$jd = if ($editor) { Join-Path $editor ($IsWindows ? 'jd.exe' : 'jd') }
if (-not $jd -or -not (Test-Path -LiteralPath $jd)) {
    throw "no installed Jackdaw $pinned (jackdaw.toml); install it with: jd editor install $pinned"
}
$abi = (& $jd extension abi | Select-Object -First 1)

# The SDK links an extension's `bevy` to its own only at the exact version
# the editor was built with; a crate asking for a newer patch moves the
# shim's lock past it, and `bsn!` then fails to compile.
$bevyVersion = '(?m)^name = "bevy"\r?\nversion = "([^"]+)"'
$sdkBevy = [regex]::Match((Get-Content -Raw (Join-Path $editor 'Cargo.lock')), $bevyVersion).Groups[1].Value

# Moves the shim's `bevy` back to the SDK's; true if it had to.
function Pin-Bevy {
    $lock = '.jackdaw/shim/Cargo.lock'
    if (-not (Test-Path -LiteralPath $lock)) { return $false }
    $current = [regex]::Match((Get-Content -Raw $lock), $bevyVersion).Groups[1].Value
    if (-not $current -or $current -eq $sdkBevy) { return $false }
    Push-Location .jackdaw/shim
    try {
        cargo update -p "bevy@$current" --precise $sdkBevy | Out-Host
        if ($LASTEXITCODE -ne 0) { throw "cannot pin the shim's bevy to $sdkBevy" }
    } finally {
        Pop-Location
    }
    $true
}

function Installed($id) {
    $line = & $jd extension list | Where-Object { $_ -like "$id`t*" } | Select-Object -First 1
    if ($line) { ($line -split "`t")[1] }
}

foreach ($ext in $extensions) {
    $id = $ext.Crates[0]
    $label = $ext.Label
    $extension = Join-Path $root "crates/$id"

    # What the extension is built from: its crates and the compiler.
    $hash = [System.Security.Cryptography.IncrementalHash]::CreateHash('SHA256')
    $hash.AppendData([System.Text.Encoding]::UTF8.GetBytes("$abi`n"))
    foreach ($crate in $ext.Crates) {
        $dir = Join-Path $root "crates/$crate"
        $files = @(Get-ChildItem -LiteralPath $dir -File -Filter 'Cargo.*') +
            @(Get-ChildItem -LiteralPath (Join-Path $dir 'src') -File -Recurse)
        foreach ($file in $files | Sort-Object FullName) {
            $name = [System.IO.Path]::GetRelativePath($root, $file.FullName).Replace('\', '/')
            $hash.AppendData([System.Text.Encoding]::UTF8.GetBytes("$name`n"))
            $hash.AppendData([System.IO.File]::ReadAllBytes($file.FullName))
        }
    }
    $digest = [System.Convert]::ToHexString($hash.GetHashAndReset()).Substring(0, 12).ToLower()
    $manifest = Get-Content -Raw (Join-Path $extension 'Cargo.toml')
    $base = [regex]::Match($manifest, '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value
    $version = "$base-dev.$digest"

    if ((Installed $id) -eq $version) {
        Write-Host "$label extension $version is up to date."
        continue
    }

    Write-Host "Building the $label extension $version (a few minutes)..."
    Push-Location $extension
    try {
        Pin-Bevy | Out-Null
        & $jd build
        # A first build creates the shim, so pin it and build again.
        if ($LASTEXITCODE -ne 0 -and (Pin-Bevy)) { & $jd build }
        if ($LASTEXITCODE -ne 0) { throw "jd build failed" }
        $bundle = Join-Path $extension ".jackdaw/$id-$version.jdext"
        & $jd extension pack --version $version --out $bundle
        if ($LASTEXITCODE -ne 0) { throw "jd extension pack failed" }
        & $jd extension install $bundle --trust-publisher
        # The install keeps its own copy of the bundle.
        Remove-Item -LiteralPath $bundle
    } finally {
        Pop-Location
    }
    # `jd extension install` exits with 0 even when it refuses a bundle.
    if ((Installed $id) -ne $version) { throw "the $label extension $version was not installed" }
    Write-Host "Installed the $label extension $version; it loads when the editor starts."
}
