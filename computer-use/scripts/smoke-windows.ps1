# P2-09: disposable desktop contracts and built-in-app smoke, with external oracles.
[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$TestExe,
    [Parameter(Mandatory=$true)][string]$WorkerExe,
    [Parameter(Mandatory=$true)][string]$FixtureExe,
    [string]$OutputDir = (Join-Path $env:TEMP 'gcu-win'),
    [ValidateSet('All','Contract','Smoke')][string]$Suite = 'All',
    [switch]$DesktopChild,
    [switch]$DiagnoseTokens,
    [switch]$NativeDiagnostics,
    [int]$ElevatedPid = 0,
    [int]$ExpectedSessionId = 0
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
if ($DesktopChild) {
    # Start before Add-Type/path validation, so initialization errors reach CI.
    New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
    Start-Transcript -Path (Join-Path $OutputDir 'desktop.log') -Force | Out-Null
    Write-Host "DESKTOP CHILD BOOT: Suite=$Suite; pid=$PID; output=$OutputDir"
    [Console]::Out.Flush()
}

. (Join-Path $PSScriptRoot 'windows-desktop.ps1')
. (Join-Path $PSScriptRoot 'windows-desktop-launch.ps1')
. (Join-Path $PSScriptRoot 'windows-desktop-watch.ps1')
. (Join-Path $PSScriptRoot 'windows-desktop-uipi.ps1')
. (Join-Path $PSScriptRoot 'windows-smoke.ps1')
Add-Type -Path (@('windows-desktop-token.cs','windows-desktop-access.cs','windows-desktop-defaults.cs','windows-desktop-process.cs','windows-desktop-fixture.cs') |
    ForEach-Object { Join-Path $PSScriptRoot $_ })

function Invoke-NativeCase {
    param([string]$Name, [int]$TimeoutMs = 0)
    # libtest exits successfully when a filter matches zero tests. Fail closed
    # on a wrong/cached artifact instead of reporting an unexecuted case passed.
    Write-DesktopProgress "NATIVE CASE BEGIN: $Name; listing ignored test"
    $listing = @(& $TestExe --list --ignored --exact $Name)
    if ($LASTEXITCODE -ne 0 -or "$($Name): test" -notin $listing) { throw "native test missing: $Name; listing=$listing" }
    # Force native stdout through PowerShell so the hidden child's transcript
    # contains the per-case diagnostics forwarded to the parent.
    Write-DesktopProgress "NATIVE CASE RUN: $Name; --ignored --nocapture --test-threads=1"
    # Hidden PowerShell has no console stderr handle. Explicitly pipe stderr so
    # inherited worker COM timings reach the transcript. Windows PowerShell 5
    # wraps native stderr as ErrorRecord; it is diagnostic text, not a cmdlet error.
    $savedPreference = $ErrorActionPreference
    try {
        if ($TimeoutMs -gt 0) {
            # System apps have individual deadlines. Terminating the test also
            # closes its kill-on-close worker job; no mutation is retried.
            $prefix = Join-Path $OutputDir $Name.Replace(':','-')
            $native = $null
            try {
                $native = Start-Process -FilePath $TestExe -ArgumentList @('--exact',$Name,'--ignored','--nocapture','--test-threads=1') `
                    -NoNewWindow -PassThru -RedirectStandardOutput ($prefix + '.stdout.log') -RedirectStandardError ($prefix + '.stderr.log')
                $null = $native.Handle
                $null = Wait-Oracle "$Name native case (no retry after timeout)" { $native.Refresh(); $native.HasExited } { param($exited) $exited } -TimeoutMs $TimeoutMs
                $caseExit = $native.ExitCode
            } finally {
                if ($null -ne $native) {
                    try { if (-not $native.HasExited) { Stop-Process -Id $native.Id -Force -ErrorAction Stop } }
                    finally { $native.Dispose() }
                }
                foreach ($log in @(($prefix + '.stdout.log'), ($prefix + '.stderr.log'))) {
                    if (Test-Path -LiteralPath $log) { Get-Content -LiteralPath $log | Out-Host }
                }
            }
        } else {
            $ErrorActionPreference = 'Continue'
            & $TestExe --exact $Name --ignored --nocapture --test-threads=1 2>&1 | ForEach-Object { Write-Host "$_" }
            $caseExit = $LASTEXITCODE
        }
    } finally {
        $ErrorActionPreference = $savedPreference
        if ($env:GCU_NATIVE_TRACE_DIR -and (Test-Path -LiteralPath $env:GCU_NATIVE_TRACE_DIR)) {
            Get-ChildItem -LiteralPath $env:GCU_NATIVE_TRACE_DIR -Filter '*.log' | ForEach-Object {
                Write-DesktopProgress "WORKER TRACE TAIL: $($_.Name); full log retained in artifacts"
                Get-Content -LiteralPath $_.FullName -Tail 60 | Out-Host
            }
            Get-ChildItem -LiteralPath $env:GCU_NATIVE_TRACE_DIR -Filter 'fixture-activation-*.jsonl' | ForEach-Object {
                Write-DesktopProgress "FIXTURE ACTIVATION TRACE: $($_.Name); observer does not change actions or Focus output"
                Get-Content -LiteralPath $_.FullName -Tail 20 | Out-Host
            }
        }
    }
    if ($caseExit -ne 0) { throw "$Name failed: exit $caseExit" }
    Write-DesktopProgress "NATIVE CASE END: $Name; exit=0"
}

New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
$OutputDir = (Resolve-Path $OutputDir).Path
$TestExe = (Resolve-Path $TestExe).Path
$WorkerExe = (Resolve-Path $WorkerExe).Path
$FixtureExe = (Resolve-Path $FixtureExe).Path
$facts = Get-DesktopTokenFacts
if ($ExpectedSessionId -eq 0 -and -not $DesktopChild) { $ExpectedSessionId = $facts.SessionId }
$diagnostic = Format-DesktopTokenFacts $facts
Write-Output "Desktop token: $diagnostic"

# UAC may be disabled on hosted runners: a Limited scheduled task can retain a
# High admin token. Explicitly construct a restricted primary token instead.
# Only this disposable CI harness lowers its token; the engine gate is unchanged.
if (-not $DesktopChild -and ($facts.IsAdmin -or $facts.IntegrityRid -gt 8192)) {
    $high = $null
    $highDirectory = $null
    $acl = @{context=$null}
    $workspace = $null
    $payload = $null
    foreach ($name in @('desktop-exit.json','desktop.log','contract.json','smoke.json','launch.json','uipi-fixture.json')) {
        Remove-Item (Join-Path $OutputDir $name) -ErrorAction SilentlyContinue
    }
    try {
        if (-not $facts.UserInteractive -or $facts.SessionId -le 0 -or $facts.Desktop -ine 'WinSta0\Default') {
            throw "parent interactive Default desktop unavailable; $diagnostic"
        }
        Write-Output "Parent token details: $($facts.TokenSummary)"
        Write-Output "Parent desktop security: $([Gcu.DesktopLauncher]::DescribeDesktopSecurity())"
        $workspace = [Gcu.DesktopLauncher]::PrepareWorkspace()
        $payload = Copy-DesktopPayload $workspace $TestExe $WorkerExe $FixtureExe
        Write-Output "Medium workspace: $workspace; source payload staged; inheritable user DACL + Medium label"
        # Upload this exact Medium-writable directory even if the step times out.
        # Keep it until artifact upload; payload/temp files are removed in finally.
        if ($env:GITHUB_ENV) {
            "GCU_DESKTOP_REPORT_DIR=$($payload.OutputDir)" | Out-File -FilePath $env:GITHUB_ENV -Encoding utf8 -Append
        }
        Write-DesktopProgress "DESKTOP REPORT DIRECTORY: $($payload.OutputDir)"
        $probeResults = [Collections.Generic.List[object]]::new()
        $variant = Invoke-DesktopLaunchLadder -DiagnoseTokens:$DiagnoseTokens -Probe {
            param($variant,$stage,$budget)
            if ($variant.RepairDesktop -and $null -eq $acl.context) {
                $acl.context = [Gcu.DesktopLauncher]::Open($ExpectedSessionId, $workspace)
                $acl.context.EnsureDesktopAccess()
            }
            $request = @{Variant=$variant;Stage=$stage;Workspace=$workspace;OutputDir=$payload.OutputDir;ExpectedSessionId=$ExpectedSessionId}
            $result = Invoke-SupervisedDesktopStage $request $workspace $payload.OutputDir $budget
            $probeResults.Add($result)
            $probeResults.ToArray() | ConvertTo-Json -Depth 10 | Set-Content -Encoding UTF8 (Join-Path $payload.OutputDir 'launch.json')
            return $result
        }
        if ($Suite -ne 'Smoke') {
            $highDirectory = Join-Path $env:TEMP ('gh' + [Guid]::NewGuid().ToString('N').Substring(0,8))
            $uipi = Start-UipiFixture -FixtureExe $FixtureExe -Directory $highDirectory -OutputDir $payload.OutputDir `
                -ParentFacts $facts -ExpectedSessionId $ExpectedSessionId
            $high = $uipi.process
            $ElevatedPid = $uipi.pid
        }
        # The selected profile is used exactly once; any timeout/failure is final.
        $request = @{Variant=$variant;Stage='suite';Workspace=$workspace;OutputDir=$payload.OutputDir;
            ExpectedSessionId=$ExpectedSessionId;TestExe=$payload.TestExe;WorkerExe=$payload.WorkerExe;
            FixtureExe=$payload.FixtureExe;Suite=$Suite;ElevatedPid=$ElevatedPid;NativeDiagnostics=$NativeDiagnostics.IsPresent}
        Write-DesktopProgress "DESKTOP SUITE BEGIN: $Suite; timeout_ms=720000; no retry"
        $result = Invoke-SupervisedDesktopStage $request $workspace $payload.OutputDir 720000
        if (-not $result.ok) { throw "suite failed; last=$($result | ConvertTo-Json -Compress -Depth 10)" }
        exit 0
    } catch {
        $reason = "restricted desktop launch failed: $($_.Exception.Message); parent $diagnostic"
        Write-Output $reason
        $reports = if ($null -ne $payload) { $payload.OutputDir } else { $OutputDir }
        if (Test-Path (Join-Path $reports 'desktop.log')) { Get-Content (Join-Path $reports 'desktop.log') -ErrorAction Continue }
        if (-not (Test-Path (Join-Path $reports 'contract.json'))) { Write-DesktopNotRun $reports $Suite $reason }
        if (-not (Test-Path (Join-Path $reports 'smoke.json'))) {
            @('Notepad','Explorer','Settings') | ForEach-Object { @{app=$_;outcome='not_run';reason=$reason} } |
                ConvertTo-Json | Set-Content -Encoding UTF8 (Join-Path $reports 'smoke.json')
            Write-Output "SMOKE SUMMARY: 0 passed, 0 skipped, 3 not_run, 0 failed; $reason"
        }
        exit 1
    } finally {
        try {
            if ($null -ne $high) {
                if (-not $high.HasExited) { Stop-Process -Id $high.Id -Force }
                $null = Wait-Oracle 'elevated fixture termination' { $high.Refresh(); $high.HasExited } { param($s) $s } -TimeoutMs 5000
                $high.Dispose()
            }
        } finally {
            try {
                if ($null -ne $highDirectory) { Remove-Item -LiteralPath $highDirectory -Recurse -Force -ErrorAction Continue }
                if ($null -ne $payload) { Copy-DesktopReports $payload.OutputDir $OutputDir }
            } finally {
                try { if ($null -ne $acl.context) { $acl.context.Dispose(); Write-DesktopProgress 'Desktop ACLs restored' } }
                finally {
                    if ($null -ne $workspace) {
                        # Preserve out/ for the always() upload, including on a
                        # step deadline. Never remove the live artifact directory.
                        Get-ChildItem -LiteralPath $workspace -Force | Where-Object { $_.Name -ne 'out' } |
                            Remove-Item -Recurse -Force -ErrorAction Continue
                    }
                }
            }
        }
    }
}

$exitCode = 1
$temp = Join-Path $env:TEMP ('gs' + [Guid]::NewGuid().ToString('N').Substring(0,8))
$smokeRows = [Collections.Generic.List[object]]::new()
foreach ($app in @('Notepad','Explorer','Settings')) {
    $smokeRows.Add(@{app=$app;outcome='not_run';reason='scenario not reached'})
}
function Save-SmokeRows {
    $smokeRows | ConvertTo-Json -Depth 6 | Set-Content -Encoding UTF8 (Join-Path $OutputDir 'smoke.json')
}
Save-SmokeRows
if (-not $DesktopChild) { Start-Transcript -Path (Join-Path $OutputDir 'desktop.log') -Force | Out-Null }
try {
    # Re-read in the child. Neither the API flags nor a parent's claim prove that
    # the effective child token is Medium, non-admin, and on the intended desktop.
    $facts = Get-DesktopTokenFacts
    $diagnostic = Format-DesktopTokenFacts $facts
    Write-Output "Verified desktop token: $diagnostic; ExpectedSessionId=$ExpectedSessionId"
    Write-DesktopProgress "DESKTOP CHILD ENTERED: Suite=$Suite; output=$OutputDir"
    # ReadFacts already queries SID/groups/privileges with Win32. whoami can
    # hang under a restricted token; only the bounded optional ladder runs it.
    Write-DesktopProgress "DESKTOP CHILD TOKEN: $($facts.TokenSummary)"
    if (-not (Test-DesktopTokenFacts $facts $ExpectedSessionId)) {
        $reason = "unelevated interactive desktop unavailable; $diagnostic; ExpectedSessionId=$ExpectedSessionId"
        Write-DesktopNotRun $OutputDir $Suite $reason
        foreach ($row in $smokeRows) { $row.reason = $reason }
        throw "not_run: $reason"
    }
    $env:GCU_DESKTOP = '1'
    $env:GCU_WORKER_EXE = $WorkerExe
    $env:GCU_NATIVE_TRACE_DIR = Join-Path $OutputDir 'native'
    New-Item -ItemType Directory -Force -Path $env:GCU_NATIVE_TRACE_DIR | Out-Null
    $env:GCU_CONTRACT_WORKER_EXE = Join-Path (Split-Path $WorkerExe -Parent) 'gcu-contract-worker.exe'
    $env:GCU_FIXTURE_EXE = $FixtureExe
    $env:GCU_ELEVATED_PID = "$ElevatedPid"
    $env:GCU_UIPI_SKIP_REASON = ''
    $uipiReport = Join-Path $OutputDir 'uipi-fixture.json'
    if ($ElevatedPid -eq 0 -and (Test-Path -LiteralPath $uipiReport)) {
        $env:GCU_UIPI_SKIP_REASON = (Get-Content -Raw -LiteralPath $uipiReport | ConvertFrom-Json).reason
    }
    $env:GCU_CONTRACT_REPORT = Join-Path $OutputDir 'contract.json'
    if ($NativeDiagnostics -and $Suite -ne 'Smoke') {
        $env:GCU_MOUSE_PROBE_REPORT = Join-Path $OutputDir 'mouse-probes.json'
        Write-DesktopProgress 'CI DIAGNOSTIC: empirical mouse hook probes; disposable desktop only; movement/XBUTTON included'
        try { Invoke-NativeCase 'unsafe_mouse_probe::live_mouse_probe_matrix' }
        catch { Write-DesktopProgress "MOUSE PROBE DIAGNOSTIC FAILED: $($_.Exception.Message); contracts still run fail-closed" }
        try { Invoke-NativeCase 'live::unsafe_background_probe::live_background_activation_probe' }
        catch { Write-DesktopProgress "BACKGROUND DIAGNOSTIC FAILED: $($_.Exception.Message); C16 equality assertion still runs" }
    }
    $suites = Invoke-DesktopSuites -Suite $Suite -Contract {
        Write-DesktopProgress 'CONTRACT SUITE START: C01-C27, C16-BUTTON, W-UIPI, W-HOTKEY; external fixture oracle; Skip reasons printed per row'
        Invoke-NativeCase 'live::live_contract_table_c01_to_c27'
        $report = Get-Content -Raw $env:GCU_CONTRACT_REPORT | ConvertFrom-Json
        if ($report.passed + $report.skipped -ne 30 -or $report.not_run -ne 0 -or $report.failed -ne 0) { throw 'incomplete native contract report' }
    } -Smoke {
        Invoke-WindowsSmoke -Rows $smokeRows -TempDir $temp -Save { Save-SmokeRows }
    }
    $exitCode = $suites.exit_code
    foreach ($failure in $suites.errors) { Write-DesktopProgress "SUITE FAILURE: $failure" }
} catch {
    Write-Output "FAILED: $($_.Exception.Message)"
    foreach ($row in $smokeRows | Where-Object { $_.outcome -eq 'not_run' }) { $row.reason = "desktop setup failed: $($_.Exception.Message)" }
} finally {
    Remove-Item -Recurse -Force $temp -ErrorAction SilentlyContinue
    $passed = @($smokeRows | Where-Object { $_.outcome -eq 'passed' }).Count
    $skipped = @($smokeRows | Where-Object { $_.outcome -eq 'skipped' }).Count
    $notRun = @($smokeRows | Where-Object { $_.outcome -eq 'not_run' }).Count
    $failed = @($smokeRows | Where-Object { $_.outcome -eq 'failed' }).Count
    Write-Output "SMOKE SUMMARY: $passed passed, $skipped skipped, $notRun not_run, $failed failed"
    $smokeRows | ConvertTo-Json -Depth 6 | Set-Content -Encoding UTF8 (Join-Path $OutputDir 'smoke.json')
    Stop-Transcript | Out-Null
    @{exit_code=$exitCode;token_facts=$facts} | ConvertTo-Json -Depth 6 | Set-Content -Encoding UTF8 (Join-Path $OutputDir 'desktop-exit.json')
}
exit $exitCode
