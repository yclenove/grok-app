param(
    [Parameter(Mandatory=$true)][ValidatePattern('^[a-z0-9-]+$')][string]$RunName,
    [ValidatePattern('^[a-z0-9-]+\.jsonl$')][string]$BuildLogName = 'app-download-close-build-20261008.jsonl',
    [ValidateSet('webview-download-close','webview-download-error-unwind','webview-download-before-start','webview-download-buffer-retirement','webview-download-publication','webview-upload','webview-upload-task','webview-upload-mcp')][string]$Mode = 'webview-download-close',
    [switch]$RejectCloseToTray
)
$ErrorActionPreference = 'Stop'
if ($RejectCloseToTray -and $Mode -ne 'webview-upload') { throw 'Negative isolation fixture is upload-only' }
$taskCloseToTray = [bool]$RejectCloseToTray
$taskErrorUnwind = $Mode -eq 'webview-download-error-unwind'
$taskExpectedExit = if ($RejectCloseToTray -or $taskErrorUnwind) { 2 } else { 0 }
$taskRepo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$taskArtifactRoot = [IO.Path]::GetFullPath((Join-Path $taskRepo 'src-tauri/target/codex-test-runtime'))
$taskRun = [IO.Path]::GetFullPath((Join-Path $taskArtifactRoot $RunName))
if (-not $taskRun.StartsWith($taskArtifactRoot + '\',[StringComparison]::OrdinalIgnoreCase)) { throw 'Invalid runtime root' }
foreach ($taskPath in @($taskRepo,(Join-Path $taskRepo 'src-tauri'),(Join-Path $taskRepo 'src-tauri/target'),$taskArtifactRoot)) {
    $taskItem = Get-Item -LiteralPath $taskPath
    if ($taskItem.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Reparse runtime ancestor' }
}
if (Test-Path -LiteralPath $taskRun) { throw 'Never overwrite App acceptance evidence' }
$taskBuildLog = Join-Path $taskArtifactRoot $BuildLogName
$taskArtifacts = @(Get-Content -LiteralPath $taskBuildLog | ForEach-Object {
    try { $taskEvent = $_ | ConvertFrom-Json } catch { return }
    if ($taskEvent.reason -eq 'compiler-artifact' -and $taskEvent.target.name -eq 'computer-use-app-probe' -and $taskEvent.executable) { $taskEvent }
})
if ($taskArtifacts.Count -ne 1) { throw 'Missing unique Cargo App probe executable' }
$taskSourceExe = [IO.Path]::GetFullPath($taskArtifacts[0].executable)
if ($taskSourceExe -ne (Join-Path $taskRepo 'src-tauri\target\debug\computer-use-app-probe.exe')) { throw 'Unexpected Cargo executable' }
if (Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -eq $taskSourceExe }) { throw 'Source App probe is running' }
if ((Get-PSDrive H).Free -lt 1GB -or (Get-PSDrive C).Free -lt 1GB) { throw 'Insufficient runtime space' }
New-Item -ItemType Directory -Path $taskRun | Out-Null
$taskExe = Join-Path $taskRun 'computer-use-app-probe.exe'
Copy-Item -LiteralPath $taskSourceExe -Destination $taskExe
$taskHash = (Get-FileHash -LiteralPath $taskExe -Algorithm SHA256).Hash
if ($taskHash -ne (Get-FileHash -LiteralPath $taskSourceExe -Algorithm SHA256).Hash) { throw 'App copy differs' }
$taskHome = Join-Path $taskRun 'home'
New-Item -ItemType Directory -Path $taskHome | Out-Null
$taskSettings = @{
    theme='dark';permissionPolicy='ask';mode='agent';onboardingDone=$true;setupSkipped=$true;proxyMode='system'
    setupWizardCompleted=$true;authSetupDeferred=$true
    sessionDataMode='independent';closeToTray=$taskCloseToTray;keepTrayForSchedules=$false;computerUseEnabled=$false
}
[IO.File]::WriteAllText((Join-Path $taskHome 'settings.json'), ($taskSettings | ConvertTo-Json) + "`n", [Text.UTF8Encoding]::new($false))
$taskReport = Join-Path $taskRun 'native-result.json'
$taskIdentity = 'com.grokapp.desktop.' + $RunName
$taskEnvNames = @('GROK_APP_HOME','GROK_HOME','GROK_APP_INSTANCE_ID','GROK_CU_APP_HARNESS','GROK_CU_APP_HARNESS_MODE','GROK_CU_APP_HARNESS_OUT')
$taskPrevious = @{}
foreach ($taskName in $taskEnvNames) { $taskPrevious[$taskName] = [Environment]::GetEnvironmentVariable($taskName,'Process') }
try {
    $env:GROK_APP_HOME = $taskHome
    $env:GROK_HOME = Join-Path $taskHome 'agent-home'
    $env:GROK_APP_INSTANCE_ID = $taskIdentity
    $env:GROK_CU_APP_HARNESS = '1'
    $env:GROK_CU_APP_HARNESS_MODE = $Mode
    $env:GROK_CU_APP_HARNESS_OUT = $taskReport
    $taskProcess = Start-Process -FilePath $taskExe -WorkingDirectory $taskRepo -WindowStyle Hidden -PassThru -RedirectStandardOutput (Join-Path $taskRun 'stdout.log') -RedirectStandardError (Join-Path $taskRun 'stderr.log')
    $taskHandle = $taskProcess.Handle
    $taskOriginalCreated = $taskProcess.StartTime.ToUniversalTime()
} finally {
    foreach ($taskName in $taskEnvNames) { [Environment]::SetEnvironmentVariable($taskName,$taskPrevious[$taskName],'Process') }
}
$taskOwner = Get-CimInstance Win32_Process -Filter "ProcessId = $($taskProcess.Id)"
if ($taskOwner -and $taskOwner.ExecutablePath -ne $taskExe) { throw 'App owner identity changed' }
if (-not $taskOwner -and -not $taskProcess.WaitForExit(0)) { throw 'Live App owner not observed' }
$taskOwners = @{}
function Record-Owner($taskItem) {
    $taskKey = "$($taskItem.ProcessId)|$($taskItem.CreationDate.ToUniversalTime().ToString('o'))"
    $taskNow = [DateTime]::UtcNow.ToString('o')
    if ($taskOwners.ContainsKey($taskKey)) {
        $taskOwners[$taskKey].lastObservedLiveUtc = $taskNow
        return
    }
    $taskRole = if ($taskItem.Name -eq 'msedgewebview2.exe') { 'browser' } else { 'app' }
    if ($taskItem.CommandLine -match '(?:^|\s)--type=([a-z-]+)(?:\s|$)') { $taskRole = $Matches[1] }
    $taskOwners[$taskKey] = [PSCustomObject]@{
        processId=$taskItem.ProcessId;parentProcessId=$taskItem.ParentProcessId
        creationUtc=$taskItem.CreationDate.ToUniversalTime().ToString('o');executable=$taskItem.ExecutablePath
        processRole=$taskRole;firstObservedLiveUtc=$taskNow;lastObservedLiveUtc=$taskNow
    }
}
function Is-ExactHomeOwner($taskItem) {
    if ($taskItem.ExecutablePath -eq $taskExe) { return $true }
    return $taskItem.Name -eq 'msedgewebview2.exe' -and $taskItem.CommandLine -and
        $taskItem.CommandLine.Replace('/','\').Contains($taskHome.Replace('/','\'))
}
if ($taskOwner) {
    Record-Owner $taskOwner
} else {
    $taskKey = "$($taskProcess.Id)|$($taskOriginalCreated.ToString('o'))"
    $taskOwners[$taskKey] = [PSCustomObject]@{
        processId=$taskProcess.Id;parentProcessId=$PID;creationUtc=$taskOriginalCreated.ToString('o')
        executable=$taskExe;recordSource='original-exited-process-handle'
        processRole='app';firstObservedLiveUtc=$null;lastObservedLiveUtc=$null
    }
}
Write-Output "Actual App probe started PID=$($taskProcess.Id) executable=$taskExe"
$taskDeadline = [DateTime]::UtcNow.AddSeconds(60)
do {
    $taskProcesses = @(Get-CimInstance Win32_Process)
    foreach ($taskItem in $taskProcesses) {
        if (Is-ExactHomeOwner $taskItem) { Record-Owner $taskItem }
    }
    $taskAdded = $true
    while ($taskAdded) {
        $taskAdded = $false
        foreach ($taskItem in $taskProcesses) {
            $taskKey = "$($taskItem.ProcessId)|$($taskItem.CreationDate.ToUniversalTime().ToString('o'))"
            if (-not $taskOwners.ContainsKey($taskKey) -and @($taskOwners.Values | Where-Object { $_.processId -eq $taskItem.ParentProcessId -and [DateTime]$_.creationUtc -le $taskItem.CreationDate.ToUniversalTime() }).Count) {
                Record-Owner $taskItem
                $taskAdded = $true
            }
        }
    }
    $taskExited = $taskProcess.WaitForExit(100)
} while (-not $taskExited -and [DateTime]::UtcNow -lt $taskDeadline)
if ($taskExited) { $taskProcess.WaitForExit() }
$taskExit = if ($taskExited) { $taskProcess.ExitCode } else { $null }
$taskAppJoinedUtc = if ($taskExited) { [DateTime]::UtcNow.ToString('o') } else { $null }
$taskDeadline = [DateTime]::UtcNow.AddSeconds(30)
do {
    # A short-lived App can exit between CIM snapshots. Include its exact-home
    # browser owners in the same original join barrier, rather than checking
    # them once after the observed descendant set has already disappeared.
    foreach ($taskItem in @(Get-CimInstance Win32_Process)) {
        if (Is-ExactHomeOwner $taskItem) { Record-Owner $taskItem }
    }
    $taskRemaining = @($taskOwners.Values | Where-Object {
        $taskCurrent = Get-CimInstance Win32_Process -Filter "ProcessId = $($_.processId)"
        $taskExact = $taskCurrent -and $taskCurrent.CreationDate.ToUniversalTime().ToString('o') -eq $_.creationUtc -and $taskCurrent.ExecutablePath -eq $_.executable
        if ($taskExact) { $_.lastObservedLiveUtc = [DateTime]::UtcNow.ToString('o') }
        $taskExact
    })
    if ($taskRemaining.Count) { Start-Sleep -Milliseconds 100 }
} while ($taskRemaining.Count -and [DateTime]::UtcNow -lt $taskDeadline)
$taskRecord = [ordered]@{
    application=$taskExe;sha256=$taskHash;appHome=$taskHome;instanceId=$taskIdentity;mode=$Mode
    originalProcessJoined=$taskExited;exitCode=$taskExit;allObservedOwnersExited=($taskRemaining.Count -eq 0)
    originalAppJoinedUtc=$taskAppJoinedUtc;ownerGateObservedUtc=[DateTime]::UtcNow.ToString('o');originalOwnerJoinBudgetSeconds=30
    expectedExitCode=$taskExpectedExit;intentionallyRejectedCloseToTray=[bool]$RejectCloseToTray
    intentionallyInjectedHeldCallbackError=$taskErrorUnwind
    owners=@($taskOwners.Values | Sort-Object processId);remaining=@($taskRemaining)
    realModel=$false;realMcp=($Mode -eq 'webview-upload-mcp');usesRealMainIpc=($Mode -ne 'webview-upload');usesRealWryDownloadCallbacks=($Mode -notin @('webview-upload','webview-upload-task','webview-upload-mcp'));taskGrantPath=($Mode -ne 'webview-upload');loginDataImported=$false
    packagedApp=$false;freshFrontend=$false;compiledEntry='debug-probe-entry-actual-app-library'
}
[IO.File]::WriteAllText((Join-Path $taskRun 'owners.json'), ($taskRecord | ConvertTo-Json -Depth 6) + "`n", [Text.UTF8Encoding]::new($false))
if (Test-Path -LiteralPath $taskReport) { Get-Content -LiteralPath $taskReport }
if (-not $taskExited -or $taskRemaining.Count) { throw 'Original App/browser owners remain; retain all artifacts' }
if ($null -eq $taskExit) { throw 'Original App exit code missing' }
if ($taskExit -ne $taskExpectedExit) { throw "Unexpected original App exit code $taskExit; expected $taskExpectedExit" }
$taskHomeRemaining = @(Get-CimInstance Win32_Process | Where-Object {
    Is-ExactHomeOwner $_
})
if ($taskHomeRemaining.Count) { throw 'Exact home still has an owner' }
$taskLoaded = Get-Content -LiteralPath (Join-Path $taskHome 'settings.json') -Raw | ConvertFrom-Json
if ($taskLoaded.sessionDataMode -ne 'independent' -or $taskLoaded.closeToTray -ne $taskCloseToTray -or $taskLoaded.keepTrayForSchedules -ne $false) { throw 'App settings fell back' }
$taskNative = Get-Content -LiteralPath $taskReport -Raw | ConvertFrom-Json
if ($taskNative.mode -ne $Mode -or $taskNative.realModel -ne $false) { throw 'Native App evidence misclassified' }
if ($RejectCloseToTray) {
    if ($taskNative.ok -ne $false -or $taskNative.error -ne 'upload gate requires an isolated independent App' -or $null -ne $taskNative.result) { throw 'Expected actual App isolation rejection missing' }
} elseif ($taskErrorUnwind) {
    if ($taskNative.ok -ne $false -or $taskNative.error -ne 'isolated fixture error while original Wry completion is held' -or $taskNative.errorUnwindVerified -ne $true) { throw 'Expected real held-callback error unwind missing' }
    $taskUnwind = $taskNative.result
    if ($taskUnwind.injectedWhileOriginalCallbackHeld -ne $true -or $taskUnwind.originalCallerJoined -ne $true -or
        $taskUnwind.originalAdapterIdle -ne $true -or $taskUnwind.originalNativeResourcesRetired -ne $true -or
        $taskUnwind.stagingEmpty -ne $true -or $taskUnwind.replayDispatched -ne $false -or
        $taskUnwind.keeperResponsive -ne $true -or $taskUnwind.lateBodyRevival -ne $false -or
        $taskUnwind.receivedBytes -le 0 -or $taskUnwind.heldOriginalCallbacks.wryProcessed -ne 0 -or
        $taskUnwind.originalCallbacks.wryProcessed -ne 1 -or $taskUnwind.ipcResult.completed -ne 2 -or
        $taskUnwind.ipcResult.failed -ne 0 -or $taskUnwind.sameLabelFreshOwner.hashVerified -ne $true) { throw 'Original error unwind postconditions missing' }
} elseif ($taskNative.ok -ne $true) { throw 'Native App evidence failed' }
if ($Mode -eq 'webview-download-buffer-retirement') {
    $taskScenarios = @($taskNative.result.scenarios)
    if ($taskScenarios.Count -ne 2 -or @($taskScenarios | Where-Object {
        $_.destination.buffered -ne $true -or $_.destination.brokerUtf16Length -lt 260 -or
        $_.realPrivateFileBytes -ne 1048576 -or $_.bufferContentVerified -ne $true -or
        $_.heldOriginalCallbacks.nativeTerminal -ne 1 -or $_.heldOriginalCallbacks.wryFinished -ne 1 -or
        $_.heldOriginalCallbacks.wryProcessed -ne 0 -or $_.originalCallbacks.wryProcessed -ne 1 -or
        $_.originalCallerJoined -ne $true -or $_.originalAdapterIdle -ne $true -or
        $_.privateBufferRemovedOnlyAfterOriginalCallback -ne $true -or $_.originalNativeResourcesRetired -ne $true -or
        $_.sameLabelFreshOwner.hashVerified -ne $true -or $_.stagingEmpty -ne $true -or
        $_.replayDispatched -ne $false -or $_.lateBodyRevival -ne $false
    }).Count) { throw 'Real held private buffer retirement postconditions missing' }
    $taskCancelled = @($taskScenarios | Where-Object { $_.cancelledAfterNativeComplete -eq $true })
    $taskSuccessful = @($taskScenarios | Where-Object { $_.cancelledAfterNativeComplete -eq $false })
    if ($taskCancelled.Count -ne 1 -or $taskCancelled[0].originalGraceMs -lt 8000 -or
        $taskSuccessful.Count -ne 1 -or $null -ne $taskSuccessful[0].originalGraceMs) { throw 'Both original caller orders were not exercised' }
} elseif ($Mode -eq 'webview-upload-mcp') {
    if ($taskNative.realMainIpc -ne $true -or $taskNative.realMcp -ne $true -or $taskNative.realAcp -ne $false -or $taskNative.taskGrantPath -ne $true -or $taskNative.nativeFixedKernel -ne $true -or $taskNative.fileChoiceSynthetic -ne $true -or $taskNative.realNativeFilePicker -ne $false) { throw 'Native MCP upload evidence misclassified' }
    if ($taskNative.result.scenariosPassed -ne 4 -or $taskNative.result.totalReceiverRequests -ne 2 -or $taskNative.result.nativeIdle -ne $true -or $taskNative.result.shippedNodeAndEmbeddedMcp -ne $true -or @($taskNative.result.scenarios | Where-Object { $_.originalCallerJoined -ne $true -or $_.originalAdapterIdle -ne $true -or $_.replayDispatched -ne $false -or $_.stopped -ne $true }).Count) { throw 'Native MCP upload postconditions missing' }
    $taskLongWait = @($taskNative.result.scenarios | Where-Object { $_.mode -eq 'wait-over-idle' })
    if ($taskLongWait.Count -ne 1 -or $taskLongWait[0].elapsedMs -lt 16000 -or $taskLongWait[0].receipt.receiverRequests -ne 1) { throw 'Native MCP original long-wait evidence missing' }
} elseif ($Mode -eq 'webview-upload-task') {
    if ($taskNative.realMainIpc -ne $true -or $taskNative.realWryDownloadCallbacks -ne $false -or $taskNative.taskGrantPath -ne $true -or $taskNative.nativeFixedKernel -ne $true -or $taskNative.fileChoiceSynthetic -ne $true -or $taskNative.realNativeFilePicker -ne $false) { throw 'Upload TaskGrant evidence misclassified' }
    if ($taskNative.result.scenariosPassed -ne 8 -or $taskNative.result.totalReceiverRequests -ne 3 -or $taskNative.result.nativeIdle -ne $true -or @($taskNative.result.scenarios | Where-Object { $_.originalCallerJoined -ne $true -or $_.originalAdapterIdle -ne $true -or $_.replayDispatched -ne $false -or $_.stopped -ne $true }).Count) { throw 'Original upload TaskGrant postconditions missing' }
} elseif ($Mode -eq 'webview-upload') {
    if ($taskNative.realMainIpc -ne $false -or $taskNative.realWryDownloadCallbacks -ne $false -or $taskNative.taskGrantPath -ne $false -or $taskNative.nativeFixedKernel -ne $true) { throw 'Upload adapter evidence misclassified' }
} elseif ($taskNative.realMainIpc -ne $true -or $taskNative.realWryDownloadCallbacks -ne $true) { throw 'Download evidence misclassified' }
Write-Output "Actual App WebView mode=$Mode negative=$($RejectCloseToTray -or $taskErrorUnwind) passed; original observed owners exited"
