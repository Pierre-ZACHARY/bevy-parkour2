# Copy only the private repository's runtime assets, never source downloads.
[CmdletBinding()]
param()
$ErrorActionPreference='Stop'
$root=Split-Path $PSScriptRoot -Parent
$source=Join-Path $root 'private-assets/assets'
if (-not (Test-Path -LiteralPath $source)) {
    if ((Test-Path (Join-Path $root 'assets/character/ninja/outfit_01.gltf')) -and
        (Test-Path (Join-Path $root 'assets/animations/yummy/locomotion.glb'))) {
        Write-Output 'Using the existing local runtime assets.'
        return
    }
    throw 'Private assets unavailable. Obtain repository access, then run git submodule update --init private-assets and git -C private-assets lfs pull --include="assets/**".'
}
foreach ($directory in @('character','animations')) {
    $inputDirectory=Join-Path $source $directory
    if (-not (Test-Path -LiteralPath $inputDirectory)) { throw "Missing private runtime directory: $directory" }
    foreach ($file in Get-ChildItem -LiteralPath $inputDirectory -Recurse -File) {
        $prefix=[byte[]]::new(128)
        $stream=[IO.File]::OpenRead($file.FullName)
        try { $count=$stream.Read($prefix,0,$prefix.Length) } finally { $stream.Dispose() }
        if ([Text.Encoding]::ASCII.GetString($prefix,0,$count).StartsWith('version https://git-lfs.github.com/spec/v1')) {
            throw 'Private LFS files are not hydrated. Run git -C private-assets lfs pull --include="assets/**".'
        }
        $relative=[IO.Path]::GetRelativePath($source,$file.FullName)
        $destination=Join-Path (Join-Path $root 'assets') $relative
        New-Item -ItemType Directory -Force (Split-Path $destination -Parent) | Out-Null
        if ((Test-Path -LiteralPath $destination) -and
            (Get-FileHash -LiteralPath $destination).Hash -eq (Get-FileHash -LiteralPath $file.FullName).Hash) { continue }
        Copy-Item -LiteralPath $file.FullName -Destination $destination -Force
    }
}
Write-Output 'Private runtime assets prepared. Authoring sources and license records stay in the private repository.'
