# Build-time ZIP creation and byte-for-byte roundtrip verification only.
# Never execute Grok.exe, an installer, a runtime binary, or publisher signing.
param([Parameter(Mandatory=$true)][string]$Stage,[Parameter(Mandatory=$true)][string]$Archive)
$ErrorActionPreference='Stop'
function Get-PortableManifestHash([string]$Root) {
    $digest=& node -e "const fs=require('node:fs'),p=require('node:path');process.stdout.write(require('node:crypto').createHash('sha256').update(fs.readFileSync(p.join(process.argv[1],'portable-files.json'))).digest('hex'));" $Root
    if($LASTEXITCODE-ne 0 -or $digest-notmatch '^[a-f0-9]{64}$'){throw 'Portable manifest hashing failed'}
    return $digest
}
function Remove-OwnedBuildPath([string]$Path) {
    # Windows PowerShell 5 Remove-Item fails on long extracted Chromium paths.
    # Node's native filesystem supports them. Only our UUID-owned paths enter.
    & node --input-type=module -e "await (await import('node:fs/promises')).rm(process.argv[1],{recursive:true,force:true,maxRetries:5,retryDelay:100});" $Path
    if($LASTEXITCODE-ne 0){throw 'Owned portable build cleanup failed'}
}
$Stage=[IO.Path]::GetFullPath($Stage)
$Archive=[IO.Path]::GetFullPath($Archive)
if($Archive.StartsWith($Stage.TrimEnd('\','/')+[IO.Path]::DirectorySeparatorChar,[StringComparison]::OrdinalIgnoreCase)){throw 'Archive must remain outside the staged image'}
$parent=Split-Path -Parent $Archive
while($parent){
    $item=Get-Item -LiteralPath $parent -Force
    if(-not $item.PSIsContainer -or ($item.Attributes-band [IO.FileAttributes]::ReparsePoint)){throw 'Archive parent must be a plain directory'}
    $parent=Split-Path -Parent $parent
}
if(Test-Path -LiteralPath $Archive){throw 'Portable archive already exists'}
$seven=(Get-Command 7z -ErrorAction SilentlyContinue).Source
if(-not $seven){$seven=Join-Path $env:ProgramFiles '7-Zip/7z.exe'}
if(-not(Test-Path -LiteralPath $seven)){throw '7-Zip is required on the Windows build machine'}
& node (Join-Path $PSScriptRoot 'stage-windows-portable.mjs') --verify $Stage
if($LASTEXITCODE-ne 0){throw 'Portable source verification failed'}
$before=Get-PortableManifestHash $Stage
$temporary="$Archive.partial-$([guid]::NewGuid()).zip"
$check="$Archive.verify-$([guid]::NewGuid())"
try {
    Push-Location -LiteralPath (Split-Path -Parent $Stage)
    try {
        # The directory name is a path, never a 7-Zip switch such as -sdel.
        & $seven a -tzip -mx=5 -y $temporary ('.\'+(Split-Path -Leaf $Stage))
        if($LASTEXITCODE-ne 0){throw 'Portable ZIP creation failed'}
    } finally {Pop-Location}
    & $seven x -y "-o$check" $temporary
    if($LASTEXITCODE-ne 0){throw 'Portable ZIP readback failed'}
    $unpacked=Join-Path $check (Split-Path -Leaf $Stage)
    & node (Join-Path $PSScriptRoot 'stage-windows-portable.mjs') --verify $unpacked
    if($LASTEXITCODE-ne 0){throw 'Portable ZIP content verification failed'}
    $after=Get-PortableManifestHash $unpacked
    if($before-ne $after){throw 'Portable ZIP manifest changed'}
    Remove-OwnedBuildPath $check
    if(Test-Path -LiteralPath $Archive){throw 'Portable output appeared during packaging'}
    Move-Item -LiteralPath $temporary -Destination $Archive
} finally {
    if(Test-Path -LiteralPath $temporary){Remove-OwnedBuildPath $temporary}
    if(Test-Path -LiteralPath $check){Remove-OwnedBuildPath $check}
}
