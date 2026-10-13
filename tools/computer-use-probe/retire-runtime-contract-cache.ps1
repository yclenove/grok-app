# Exact obsolete generations created by the 2026-10-13 R2 contract batch.
$ErrorActionPreference = 'Stop'
$cacheRoot = 'H:\aicoding\grok-app-computer-use\src-tauri\target\debug\incremental'
$cacheSpecs = @(
    @{
        Retire = 'grok_computer_use_core-16iqferb236aw\s-hn5hjlm61l-0e03fuz-5bakm9qxy92y3mo4kc2j4hnyc'
        Keep = 'grok_computer_use_core-16iqferb236aw\s-hn5hr6m1i4-10lvdtr-37e6hdrpajn772mwezubkwk8r'
        Stamp = '2026-10-12T19:53:25.8526485Z'
        Count = 259
        Bytes = 549503932L
    },
    @{
        Retire = 'grok_computer_use_core-12800x0h20pk4\s-hn5hpriaro-1mcot2o-1wu6lw2ylalqa1xlo7n4g3js1'
        Keep = 'grok_computer_use_core-12800x0h20pk4\s-hn5hu9g25q-084k2oq-bn51lqj5z33zljql5qefx6q4v'
        Stamp = '2026-10-12T19:59:38.8067319Z'
        Count = 4
        Bytes = 53697615L
    }
)

function Assert-OrdinaryCachePath([string] $LiteralPath) {
    $resolved = (Resolve-Path -LiteralPath $LiteralPath).ProviderPath
    if (-not $resolved.StartsWith($cacheRoot + '\', [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Cache target escaped the exact task artifact root'
    }
    $ancestor = Get-Item -LiteralPath $resolved -Force
    while ($null -ne $ancestor) {
        if (($ancestor.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw 'Cache target/ancestor is a reparse point'
        }
        $ancestor = $ancestor.Parent
    }
    return $resolved
}

function Assert-OwnersExited {
    # Conservative: an uncertain compiler owner defers cleanup, never gets stopped.
    $owners = @(Get-CimInstance Win32_Process -Filter "Name='cargo.exe' OR Name='rustc.exe' OR Name='grok_computer_use_core-469532688ae03771.exe' OR Name='runtime_sources_cli-015fca419f4a78d1.exe'")
    if ($owners.Count -ne 0) { throw 'Compile/test owner live or uncertain; defer retirement' }
}

Assert-OwnersExited
$verified = @()
foreach ($spec in $cacheSpecs) {
    $target = Assert-OrdinaryCachePath (Join-Path $cacheRoot $spec.Retire)
    $keeper = Assert-OrdinaryCachePath (Join-Path $cacheRoot $spec.Keep)
    $targetInfo = Get-Item -LiteralPath $target -Force
    $keepInfo = Get-Item -LiteralPath $keeper -Force
    if (-not $targetInfo.PSIsContainer -or -not $keepInfo.PSIsContainer -or
        $targetInfo.LastWriteTimeUtc.ToString('o') -ne $spec.Stamp -or
        $keepInfo.LastWriteTimeUtc -le $targetInfo.LastWriteTimeUtc) {
        throw 'Generation identity or retained successor changed'
    }
    $entries = @(Get-ChildItem -LiteralPath $target -Recurse -Force)
    if (@($entries | Where-Object { ($_.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 }).Count -ne 0) {
        throw 'Cache generation contains a reparse point'
    }
    $bytes = ($entries | Where-Object { -not $_.PSIsContainer } | Measure-Object Length -Sum).Sum
    if ($entries.Count -ne $spec.Count -or $bytes -ne $spec.Bytes) { throw 'Cache inventory changed' }
    $verified += [PSCustomObject]@{Path=$target;RetainedSuccessor=$keeper;Entries=$entries.Count;LogicalBytes=[long]$bytes}
}
Assert-OwnersExited
$freeBefore = (Get-PSDrive -Name H).Free
foreach ($generation in $verified) {
    Remove-Item -LiteralPath $generation.Path -Recurse -Force
    if (Test-Path -LiteralPath $generation.Path) { throw 'Generation retirement did not finish' }
    if (-not (Test-Path -LiteralPath $generation.RetainedSuccessor)) { throw 'Retained successor missing' }
}
$freeAfter = (Get-PSDrive -Name H).Free
[PSCustomObject]@{
    Removed = $verified
    LogicalBytes = [long](($verified | Measure-Object LogicalBytes -Sum).Sum)
    ObservedHFreeSpaceDeltaBytes = [long]($freeAfter - $freeBefore)
    MeasurementScope = 'Immediate drive free-space observation; hardlinks/concurrent writes can differ from logical bytes'
    Regenerate = @(
        'cargo test --manifest-path src-tauri/Cargo.toml --locked -p grok-computer-use-core --lib -- --test-threads=1'
        'cargo clippy --manifest-path src-tauri/Cargo.toml --locked -p grok-computer-use-core --all-targets -- -D warnings'
    )
    Retained = 'Current binaries/generations/dependencies, immutable seed, source archives, Linux06 snapshot/target, failed/passed evidence and user App/home'
} | ConvertTo-Json -Depth 5
