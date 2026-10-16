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
Add-Type -Path (@('windows-desktop-token.cs','windows-desktop-access.cs','windows-desktop-defaults.cs','windows-desktop-process.cs','windows-desktop-fixture.cs') |
    ForEach-Object { Join-Path $PSScriptRoot $_ })

function Invoke-NativeCase {
    param([string]$Name)
    # libtest exits successfully when a filter matches zero tests. Fail closed
    # on a wrong/cached artifact instead of reporting an unexecuted case passed.
    Write-DesktopProgress "NATIVE CASE BEGIN: $Name; listing ignored test"
    $listing = @(& $TestExe --list --ignored --exact $Name)
    if ($LASTEXITCODE -ne 0 -or "$($Name): test" -notin $listing) { throw "native test missing: $Name; listing=$listing" }
    # Force native stdout through PowerShell so the hidden child's transcript
    # contains the per-case diagnostics forwarded to the parent.
    Write-DesktopProgress "NATIVE CASE RUN: $Name; --ignored --nocapture --test-threads=1"
    & $TestExe --exact $Name --ignored --nocapture --test-threads=1 | Out-Host
    if ($LASTEXITCODE -ne 0) { throw "$Name failed: exit $LASTEXITCODE" }
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
    $acl = @{context=$null}
    $workspace = $null
    $payload = $null
    foreach ($name in @('desktop-exit.json','desktop.log','contract.json','smoke.json','launch.json')) {
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
            $highExe = Join-Path $workspace 'hi.exe'
            Copy-Item $FixtureExe $highExe
            $savedState = $env:GCU_FIXTURE_STATE
            $savedActivation = $env:GCU_FIXTURE_NO_ACTIVATE
            try {
                $env:GCU_FIXTURE_STATE = Join-Path $payload.OutputDir 'high-state.json'
                $env:GCU_FIXTURE_NO_ACTIVATE = '1'
                $high = [Gcu.DesktopLauncher]::StartUnactivatedFixture($highExe)
            } finally { $env:GCU_FIXTURE_STATE = $savedState; $env:GCU_FIXTURE_NO_ACTIVATE = $savedActivation }
            $null = Wait-Oracle 'elevated fixture window' { $high.Refresh(); @{ exited=$high.HasExited; hwnd=$high.MainWindowHandle.ToInt64() } } { param($s) -not $s.exited -and $s.hwnd -ne 0 }
            $ElevatedPid = $high.Id
            $highToken = [Gcu.DesktopLauncher]::ReadFixtureToken($ElevatedPid)
            Write-DesktopProgress "HIGH FIXTURE TOKEN: $highToken"
            if ($highToken.IntegrityRid -lt 12288 -or $highToken.SessionId -ne $ExpectedSessionId) {
                throw "elevated fixture token invalid: $highToken; parent $diagnostic"
            }
        }
        # The selected profile is used exactly once; any timeout/failure is final.
        $request = @{Variant=$variant;Stage='suite';Workspace=$workspace;OutputDir=$payload.OutputDir;
            ExpectedSessionId=$ExpectedSessionId;TestExe=$payload.TestExe;WorkerExe=$payload.WorkerExe;
            FixtureExe=$payload.FixtureExe;Suite=$Suite;ElevatedPid=$ElevatedPid}
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
$owned = [Collections.Generic.List[Diagnostics.Process]]::new()
$shell = $null
$folderWindows = @()
$temp = Join-Path $env:TEMP ('gs' + [Guid]::NewGuid().ToString('N').Substring(0,8))
$smokeRows = [Collections.Generic.List[object]]::new()
foreach ($app in @('Notepad','Explorer','Settings')) {
    $smokeRows.Add(@{app=$app;outcome='not_run';reason='scenario not reached'})
}
function Save-SmokeRows {
    $smokeRows | ConvertTo-Json -Depth 6 | Set-Content -Encoding UTF8 (Join-Path $OutputDir 'smoke.json')
}
Save-SmokeRows
$activeSmoke = $null
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
    $env:GCU_CONTRACT_WORKER_EXE = Join-Path (Split-Path $WorkerExe -Parent) 'gcu-contract-worker.exe'
    $env:GCU_FIXTURE_EXE = $FixtureExe
    $env:GCU_ELEVATED_PID = "$ElevatedPid"
    $env:GCU_CONTRACT_REPORT = Join-Path $OutputDir 'contract.json'
    if ($Suite -ne 'Smoke') {
        Write-DesktopProgress 'CONTRACT SUITE START: C01-C27, W-UIPI, W-HOTKEY; external fixture oracle; Skip reasons printed per row'
        Invoke-NativeCase 'live::live_contract_table_c01_to_c27'
        $report = Get-Content -Raw $env:GCU_CONTRACT_REPORT | ConvertFrom-Json
        if ($report.passed + $report.skipped -ne 29 -or $report.not_run -ne 0 -or $report.failed -ne 0) { throw 'incomplete native contract report' }
    }
    if ($Suite -ne 'Contract') {
        Add-Type -AssemblyName UIAutomationClient
        Add-Type -AssemblyName UIAutomationTypes
        New-Item -ItemType Directory -Path $temp | Out-Null
        $activeSmoke = $smokeRows[0]
        Write-DesktopProgress 'SMOKE BEGIN: Notepad write/save; external file oracle'
        $file = Join-Path $temp 'note.txt'
        [IO.File]::WriteAllText($file,'',[Text.UTF8Encoding]::new($false))
        $notepad = Start-Process notepad.exe -ArgumentList ('"'+$file+'"') -PassThru
        $owned.Add($notepad)
        $null = Wait-Oracle 'Notepad window' { $notepad.Refresh(); $notepad.MainWindowHandle.ToInt64() } { param($h) $h -ne 0 }
        $env:GCU_SMOKE_PID = "$($notepad.Id)"
        Remove-Item Env:GCU_SMOKE_HWND -ErrorAction SilentlyContinue
        Invoke-NativeCase 'smoke::live_notepad_write_save'
        $unicodeExpected = 'GCU ' + [char]0x4e2d + [char]0x6587 + ' ' + [char]::ConvertFromUtf32(0x1f469) + [char]0x200d + [char]::ConvertFromUtf32(0x1f4bb)
        $null = Wait-Oracle 'Notepad saved exact Unicode bytes' { [IO.File]::ReadAllText($file) } { param($v) $v -ceq $unicodeExpected }
        $activeSmoke.outcome = 'passed'
        $activeSmoke.reason = 'saved file content'
        Save-SmokeRows
        $activeSmoke = $smokeRows[1]
        Write-DesktopProgress 'SMOKE BEGIN: Explorer navigation; external Shell.Application oracle'

        $source = New-Item -ItemType Directory -Path (Join-Path $temp 'a')
        $dest = New-Item -ItemType Directory -Path (Join-Path $temp 'b')
        $shell = New-Object -ComObject Shell.Application
        $prior = @($shell.Windows() | ForEach-Object { $_.HWND })
        Start-Process explorer.exe -ArgumentList ('/n,"'+$source.FullName+'"') | Out-Null
        $explorer = Wait-Oracle 'new Explorer folder window' {
            @($shell.Windows() | Where-Object { $_.HWND -notin $prior -and $_.Document.Folder.Self.Path -eq $source.FullName })
        } { param($ws) $null -ne $ws -and @($ws).Count -eq 1 }
        $explorer = @($explorer)[0]
        $folderWindows += $explorer
        $env:GCU_SMOKE_HWND = "$($explorer.HWND)"
        $root = [Windows.Automation.AutomationElement]::FromHandle([IntPtr]$explorer.HWND)
        $env:GCU_SMOKE_PID = "$($root.Current.ProcessId)"
        $env:GCU_SMOKE_DEST = $dest.FullName
        Invoke-NativeCase 'smoke::live_explorer_navigate'
        $null = Wait-Oracle 'Explorer exact destination' { $explorer.Document.Folder.Self.Path } { param($p) $p -eq $dest.FullName }
        $activeSmoke.outcome = 'passed'
        $activeSmoke.reason = 'Shell.Application folder path'
        Save-SmokeRows
        $activeSmoke = $smokeRows[2]
        Write-DesktopProgress 'SMOKE BEGIN: Settings search; independent UIA oracle'

        $previousSettings = @(Get-Process SystemSettings -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
        Start-Process 'ms-settings:' | Out-Null
        $settingsProcess = Wait-Oracle 'Settings process' {
            @(Get-Process SystemSettings -ErrorAction SilentlyContinue | Where-Object { $_.Id -notin $previousSettings })
        } { param($ps) $null -ne $ps -and @($ps).Count -eq 1 }
        $settingsProcess = @($settingsProcess)[0]
        $owned.Add($settingsProcess)
        $condition = [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ProcessIdProperty,$settingsProcess.Id)
        $root = Wait-Oracle 'Settings UIA window' {
            $windows = [Windows.Automation.AutomationElement]::RootElement.FindAll([Windows.Automation.TreeScope]::Children,[Windows.Automation.Condition]::TrueCondition)
            foreach ($window in $windows) {
                if ($window.Current.ProcessId -eq $settingsProcess.Id -or $null -ne $window.FindFirst([Windows.Automation.TreeScope]::Descendants,$condition)) { $window; break }
            }
        } { param($e) $null -ne $e }
        $settingsBefore = @{}
        $initialNodes = $root.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)
        foreach ($node in $initialNodes) { $settingsBefore[($node.GetRuntimeId() -join '.')] = $true }
        $env:GCU_SMOKE_HWND = "$($root.Current.NativeWindowHandle)"
        $env:GCU_SMOKE_PID = "$($settingsProcess.Id)"
        Invoke-NativeCase 'smoke::live_settings_search'
        $null = Wait-Oracle 'Settings query and displayed results' {
            $nodes = $root.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)
            $query = $false
            $results = @()
            foreach ($node in $nodes) {
                $pattern = $null
                if ($node.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern,[ref]$pattern)) {
                    if ($pattern.Current.Value -ceq 'display') { $query = $true }
                }
                if (-not $node.Current.IsOffscreen -and $node.Current.ControlType -ne [Windows.Automation.ControlType]::Edit -and $node.Current.Name -match 'Display' -and -not $settingsBefore.ContainsKey(($node.GetRuntimeId() -join '.'))) { $results += $node.Current.Name }
            }
            @{query=$query;results=$results}
        } { param($s) $s.query -and $s.results.Count -gt 0 }
        $activeSmoke.outcome = 'passed'
        $activeSmoke.reason = 'independent UIA query and results'
        Save-SmokeRows
        $activeSmoke = $null
    }
    $exitCode = 0
} catch {
    Write-Output "FAILED: $($_.Exception.Message)"
    if ($null -ne $activeSmoke) {
        $activeSmoke.outcome = 'failed'
        $activeSmoke.reason = $_.Exception.Message
    }
} finally {
    foreach ($window in $folderWindows) { try { $window.Quit() } catch { Write-Output "Explorer cleanup: $_" } }
    foreach ($process in $owned) { if (-not $process.HasExited) { Stop-Process -Id $process.Id -Force -ErrorAction Continue } }
    Remove-Item -Recurse -Force $temp -ErrorAction SilentlyContinue
    $passed = @($smokeRows | Where-Object { $_.outcome -eq 'passed' }).Count
    $notRun = @($smokeRows | Where-Object { $_.outcome -eq 'not_run' }).Count
    $failed = @($smokeRows | Where-Object { $_.outcome -eq 'failed' }).Count
    Write-Output "SMOKE SUMMARY: $passed passed, 0 skipped, $notRun not_run, $failed failed"
    $smokeRows | ConvertTo-Json -Depth 6 | Set-Content -Encoding UTF8 (Join-Path $OutputDir 'smoke.json')
    Stop-Transcript | Out-Null
    @{exit_code=$exitCode;token_facts=$facts} | ConvertTo-Json -Depth 6 | Set-Content -Encoding UTF8 (Join-Path $OutputDir 'desktop-exit.json')
}
exit $exitCode
