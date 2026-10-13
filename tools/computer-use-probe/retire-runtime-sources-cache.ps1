# Only obsolete incremental generations created by this source-audit batch.
$artifactRoot = 'H:\aicoding\grok-app-computer-use\src-tauri\target\debug\incremental'
$specs = @(
    @{
        Old='grok_computer_use_core-16iqferb236aw\s-hn4mgfw0pv-0xc2xku-8armghsl03ubg3nkkiso5y7uc'
        Keep='grok_computer_use_core-16iqferb236aw\s-hn5ab95hqo-1rpzeiy-e56mzc61gju2nc2vpuqluwob6'
        Stamp='2026-10-12T01:05:37.5224928Z'; Files=259; Bytes=548903474L
    },
    @{
        Old='grok_computer_use_core-06q77qy02tfzi\s-hn4m6xv7xd-0uihse0-0mk1xlc3bd7f4xnfs183363le'
        Keep='grok_computer_use_core-06q77qy02tfzi\s-hn5af7cpnm-0m0ddz2-1d4eccxs83oaxbv577fotnfq7'
        Stamp='2026-10-12T00:56:12.4578088Z'; Files=260; Bytes=346958373L
    }
)

function Assert-OrdinaryGeneration([string] $LiteralPath) {
    $resolved = (Resolve-Path -LiteralPath $LiteralPath -ErrorAction Stop).ProviderPath
    if (-not $resolved.StartsWith($artifactRoot + '\', [StringComparison]::OrdinalIgnoreCase)) {
        throw 'Generation escaped the exact task artifact root'
    }
    $ancestor = Get-Item -LiteralPath $resolved -Force -ErrorAction Stop
    while ($null -ne $ancestor) {
        if (($ancestor.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Reparse target/ancestor' }
        $ancestor = $ancestor.Parent
    }
    return $resolved
}

function Assert-OwnersRetired {
    $owners = @(Get-CimInstance Win32_Process -ErrorAction Stop | Where-Object {
        $_.Name -match '^(cargo|rustc|grok_computer_use_core|runtime_sources_cli|cu-prepare-runtime)'
    })
    if ($owners.Count -ne 0) { throw 'Build/test/audit owner live or uncertain; defer, never stop it' }
}

Assert-OwnersRetired
$verified = @()
foreach ($spec in $specs) {
    $target = Assert-OrdinaryGeneration (Join-Path $artifactRoot $spec.Old)
    $keeper = Assert-OrdinaryGeneration (Join-Path $artifactRoot $spec.Keep)
    $info = Get-Item -LiteralPath $target -Force -ErrorAction Stop
    $keepInfo = Get-Item -LiteralPath $keeper -Force -ErrorAction Stop
    if (-not $info.PSIsContainer -or -not $keepInfo.PSIsContainer -or
        $info.LastWriteTimeUtc.ToString('o') -ne $spec.Stamp -or $keepInfo.LastWriteTimeUtc -le $info.LastWriteTimeUtc) {
        throw 'Generation identity/retained successor changed'
    }
    $entries = @(Get-ChildItem -LiteralPath $target -Recurse -Force -ErrorAction Stop)
    if (@($entries | Where-Object {($_.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0}).Count -ne 0) {
        throw 'Generation contains reparse points'
    }
    $files = @($entries | Where-Object {-not $_.PSIsContainer})
    $bytes = [long](($files | Measure-Object Length -Sum).Sum)
    if ($files.Count -ne $spec.Files -or $bytes -ne $spec.Bytes) { throw 'Exact generation inventory changed' }
    $verified += [PSCustomObject]@{Path=$target;RetainedSuccessor=$keeper;Files=$files.Count;LogicalBytes=$bytes}
}
Assert-OwnersRetired
$freeBefore = (Get-PSDrive -Name H -ErrorAction Stop).Free
foreach ($target in $verified) {
    Remove-Item -LiteralPath $target.Path -Recurse -Force -ErrorAction Stop
    if (Test-Path -LiteralPath $target.Path) { throw 'Retirement incomplete' }
    if (-not (Test-Path -LiteralPath $target.RetainedSuccessor)) { throw 'Current successor missing' }
}
$freeAfter = (Get-PSDrive -Name H -ErrorAction Stop).Free
[PSCustomObject]@{
    Removed=$verified
    LogicalBytes=[long](($verified | Measure-Object LogicalBytes -Sum).Sum)
    ObservedHFreeSpaceDeltaBytes=[long]($freeAfter - $freeBefore)
    MeasurementScope='Immediate free-space observation, not exclusive attribution; hardlinks/concurrent activity may differ'
    Regenerate='cargo test --manifest-path src-tauri/Cargo.toml --locked -p grok-computer-use-core --lib; cargo test --manifest-path src-tauri/Cargo.toml --locked -p grok-computer-use-core --test runtime_sources_cli'
    Retained='Current compiled binaries/dependencies/successors, immutable seeds, candidate source caches, Linux snapshots, all logs/first failures, user package/login/history'
    Deferred='Linux originals while live and candidate dependencies needed for next source/maintenance audit'
} | ConvertTo-Json -Depth 5
