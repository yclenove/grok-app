# Retire only the two superseded incremental generations from this exact batch.
$cacheRoot = 'H:\aicoding\grok-app-computer-use\src-tauri\target-cu-review\debug\incremental'
$cacheSpecs = @(
    @{
        Retire = 'grok_computer_use_core-16iqferb236aw\s-hn4lfc6934-0oymun8-aixh517ix40pvnl54u64uipac'
        Keep = 'grok_computer_use_core-16iqferb236aw\s-hn4lnl8trs-1yovlc7-blmt3sys6q9lu6desdtv0nnzk'
        Stamp = '2026-10-12T00:28:16.3674437Z'
        Count = 259
        Bytes = 548573095L
    },
    @{
        Retire = 'grok_computer_use_core-12800x0h20pk4\s-hn4lk3lmw6-0a6a3dw-9lsi0x9fkca65pejhdx0j5ptr'
        Keep = 'grok_computer_use_core-12800x0h20pk4\s-hn4lqlbmst-0tdqukd-du0sm3r76nc9nbvms6xz0scav'
        Stamp = '2026-10-12T00:33:00.3266358Z'
        Count = 4
        Bytes = 52294454L
    }
)

function Assert-OrdinaryCachePath([string] $LiteralPath) {
    $resolved = (Resolve-Path -LiteralPath $LiteralPath -ErrorAction Stop).ProviderPath
    if (-not $resolved.StartsWith($cacheRoot + '\', [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Cache target escaped its exact artifact root'
    }
    $ancestor = Get-Item -LiteralPath $resolved -Force -ErrorAction Stop
    while ($null -ne $ancestor) {
        if (($ancestor.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) {
            throw 'Cache target/ancestor is a reparse point'
        }
        $ancestor = $ancestor.Parent
    }
    return $resolved
}

$compileOwners = @(Get-CimInstance Win32_Process -Filter "Name='cargo.exe' OR Name='rustc.exe' OR Name='grok_computer_use_core-469532688ae03771.exe'" -ErrorAction Stop)
if ($compileOwners.Count -ne 0) { throw 'Compile/test owner is live or uncertain; defer retirement' }
$verified = @()
foreach ($spec in $cacheSpecs) {
    $target = Assert-OrdinaryCachePath (Join-Path $cacheRoot $spec.Retire)
    $keeper = Assert-OrdinaryCachePath (Join-Path $cacheRoot $spec.Keep)
    $targetInfo = Get-Item -LiteralPath $target -Force -ErrorAction Stop
    $keepInfo = Get-Item -LiteralPath $keeper -Force -ErrorAction Stop
    if (-not $targetInfo.PSIsContainer -or -not $keepInfo.PSIsContainer -or
        $targetInfo.LastWriteTimeUtc.ToString('o') -ne $spec.Stamp -or
        $keepInfo.LastWriteTimeUtc -le $targetInfo.LastWriteTimeUtc) {
        throw 'Generation identity or retained successor changed'
    }
    $entries = @(Get-ChildItem -LiteralPath $target -Recurse -Force -ErrorAction Stop)
    if (@($entries | Where-Object { ($_.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0 }).Count -ne 0) {
        throw 'Cache generation contains a reparse point'
    }
    $bytes = ($entries | Where-Object { -not $_.PSIsContainer } | Measure-Object -Property Length -Sum).Sum
    if ($entries.Count -ne $spec.Count -or $bytes -ne $spec.Bytes) { throw 'Cache generation inventory changed' }
    $verified += [PSCustomObject]@{Path=$target;RetainedSuccessor=$keeper;Entries=$entries.Count;LogicalBytes=[long]$bytes}
}
# Recheck owners immediately before the scoped removals; never stop any process.
if (@(Get-CimInstance Win32_Process -Filter "Name='cargo.exe' OR Name='rustc.exe' OR Name='grok_computer_use_core-469532688ae03771.exe'" -ErrorAction Stop).Count -ne 0) {
    throw 'Compile/test owner appeared; defer retirement'
}
$freeBefore = (Get-PSDrive -Name H -ErrorAction Stop).Free
foreach ($generation in $verified) {
    Remove-Item -LiteralPath $generation.Path -Recurse -Force -ErrorAction Stop
    if (Test-Path -LiteralPath $generation.Path) { throw 'Generation retirement did not finish' }
    if (-not (Test-Path -LiteralPath $generation.RetainedSuccessor)) { throw 'Retained successor is missing' }
}
$freeAfter = (Get-PSDrive -Name H -ErrorAction Stop).Free
[PSCustomObject]@{
    Removed = $verified
    LogicalBytes = [long](($verified | Measure-Object -Property LogicalBytes -Sum).Sum)
    ObservedHFreeSpaceDeltaBytes = [long]($freeAfter - $freeBefore)
    MeasurementScope = 'Immediate drive free-space observation, not exclusive attribution; hardlinks/concurrent writes may differ'
    Regenerate = @(
        'CARGO_TARGET_DIR=src-tauri/target-cu-review cargo test --manifest-path src-tauri/computer-use-core/Cargo.toml --locked --lib'
        'CARGO_TARGET_DIR=src-tauri/target-cu-review cargo clippy --manifest-path src-tauri/computer-use-core/Cargo.toml --locked --all-targets -- -D warnings'
    )
    Retained = 'Current test binary/generations/dependencies, both Jammy snapshots/logs, source archives, immutable seed, user App/home'
} | ConvertTo-Json -Depth 5
