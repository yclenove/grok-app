# P2-09: disposable desktop contracts and built-in-app smoke, with external oracles.
[CmdletBinding()]
param(
    [Parameter(Mandatory=$true)][string]$TestExe,
    [Parameter(Mandatory=$true)][string]$WorkerExe,
    [Parameter(Mandatory=$true)][string]$FixtureExe,
    [string]$OutputDir = (Join-Path $env:TEMP 'gcu-win'),
    [ValidateSet('All','Contract','Smoke')][string]$Suite = 'All',
    [switch]$DesktopChild,
    [int]$ElevatedPid = 0
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

# Injectable monotonic clock/wait: every wait is bounded and tests an external
# predicate. No unconditional startup sleeps or retries of driver mutations.
function Wait-Oracle {
    param([string]$Label, [scriptblock]$Read, [scriptblock]$Accept,
        [int]$TimeoutMs = 20000,
        [scriptblock]$Now = { [double][Diagnostics.Stopwatch]::GetTimestamp() * 1000 / [Diagnostics.Stopwatch]::Frequency },
        [scriptblock]$Tick = { Start-Sleep -Milliseconds 50 })
    $start = & $Now
    $last = $null
    do {
        try {
            $last = & $Read
            if (& $Accept $last) { return $last }
        } catch { $last = $_.Exception.Message }
        if ((& $Now) - $start -ge $TimeoutMs) {
            throw "$Label timeout; last oracle=$($last | ConvertTo-Json -Compress -Depth 8)"
        }
        & $Tick
    } while ($true)
}

function Invoke-NativeCase {
    param([string]$Name)
    # libtest exits successfully when a filter matches zero tests. Fail closed
    # on a wrong/cached artifact instead of reporting an unexecuted case passed.
    $listing = @(& $TestExe --list --ignored --exact $Name)
    if ($LASTEXITCODE -ne 0 -or "$($Name): test" -notin $listing) { throw "native test missing: $Name; listing=$listing" }
    & $TestExe --exact $Name --ignored --nocapture --test-threads=1
    if ($LASTEXITCODE -ne 0) { throw "$Name failed: exit $LASTEXITCODE" }
}

New-Item -ItemType Directory -Force -Path $OutputDir | Out-Null
$OutputDir = (Resolve-Path $OutputDir).Path
$TestExe = (Resolve-Path $TestExe).Path
$WorkerExe = (Resolve-Path $WorkerExe).Path
$FixtureExe = (Resolve-Path $FixtureExe).Path
$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$admin = ([Security.Principal.WindowsPrincipal]$identity).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

# Hosted Windows runners launch job steps elevated. This trusted test launcher
# uses the existing interactive user's LIMITED scheduled-task token. The engine
# keeps its production High/UIAccess rejection; no credentials or UAC clicks.
if ($admin -and -not $DesktopChild) {
    $task = 'gcu-' + [Guid]::NewGuid().ToString('N').Substring(0,8)
    $high = $null
    $done = Join-Path $OutputDir 'desktop-exit.json'
    Remove-Item $done -ErrorAction SilentlyContinue
    try {
        if ($Suite -ne 'Smoke') {
            $highExe = Join-Path $OutputDir 'hi.exe'
            Copy-Item $FixtureExe $highExe
            $savedState = $env:GCU_FIXTURE_STATE
            try {
                $env:GCU_FIXTURE_STATE = Join-Path $OutputDir 'high-state.json'
                $high = Start-Process -FilePath $highExe -PassThru
            } finally { $env:GCU_FIXTURE_STATE = $savedState }
            $null = Wait-Oracle 'elevated fixture window' { $high.Refresh(); @{ exited=$high.HasExited; hwnd=$high.MainWindowHandle.ToInt64() } } { param($s) -not $s.exited -and $s.hwnd -ne 0 }
            $ElevatedPid = $high.Id
        }
        $arguments = @('-NoProfile','-ExecutionPolicy','Bypass','-File',('"'+$PSCommandPath+'"'),'-DesktopChild',
            '-TestExe',('"'+$TestExe+'"'),'-WorkerExe',('"'+$WorkerExe+'"'),'-FixtureExe',('"'+$FixtureExe+'"'),
            '-OutputDir',('"'+$OutputDir+'"'),'-Suite',$Suite,'-ElevatedPid',$ElevatedPid) -join ' '
        $action = New-ScheduledTaskAction -Execute "$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe" -Argument $arguments
        $principal = New-ScheduledTaskPrincipal -UserId $identity.Name -LogonType Interactive -RunLevel Limited
        $settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Minutes 15)
        Register-ScheduledTask -TaskName $task -Action $action -Principal $principal -Settings $settings | Out-Null
        Start-ScheduledTask -TaskName $task
        $result = Wait-Oracle 'limited interactive suite' {
            if (Test-Path $done) { Get-Content -Raw $done | ConvertFrom-Json }
            else { @{ task=(Get-ScheduledTask -TaskName $task).State.ToString(); result=(Get-ScheduledTaskInfo -TaskName $task).LastTaskResult } }
        } { param($s) $null -ne $s.PSObject.Properties['exit_code'] } -TimeoutMs 900000
        Get-Content (Join-Path $OutputDir 'desktop.log')
        exit ([int]$result.exit_code)
    } finally {
        Stop-ScheduledTask -TaskName $task -ErrorAction SilentlyContinue
        Unregister-ScheduledTask -TaskName $task -Confirm:$false -ErrorAction SilentlyContinue
        if ($null -ne $high -and -not $high.HasExited) { Stop-Process -Id $high.Id -Force }
        Remove-Item (Join-Path $OutputDir 'hi.exe') -ErrorAction SilentlyContinue
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
$activeSmoke = $null
Start-Transcript -Path (Join-Path $OutputDir 'desktop.log') -Force | Out-Null
try {
    if ($admin -or -not [Environment]::UserInteractive -or [Diagnostics.Process]::GetCurrentProcess().SessionId -eq 0) {
        Write-Output 'SUMMARY: 0 passed, 0 skipped, 29 not_run, 0 failed; unelevated interactive desktop unavailable'
        throw 'not_run: runner needs a limited token on an interactive Default desktop'
    }
    $env:GCU_DESKTOP = '1'
    $env:GCU_WORKER_EXE = $WorkerExe
    $env:GCU_FIXTURE_EXE = $FixtureExe
    $env:GCU_ELEVATED_PID = "$ElevatedPid"
    $env:GCU_CONTRACT_REPORT = Join-Path $OutputDir 'contract.json'
    if ($Suite -ne 'Smoke') {
        Invoke-NativeCase 'live::live_contract_table_c01_to_c27'
        $report = Get-Content -Raw $env:GCU_CONTRACT_REPORT | ConvertFrom-Json
        if ($report.passed + $report.skipped -ne 29 -or $report.not_run -ne 0 -or $report.failed -ne 0) { throw 'incomplete native contract report' }
    }
    if ($Suite -ne 'Contract') {
        Add-Type -AssemblyName UIAutomationClient
        Add-Type -AssemblyName UIAutomationTypes
        New-Item -ItemType Directory -Path $temp | Out-Null
        $activeSmoke = $smokeRows[0]
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
        $activeSmoke = $smokeRows[1]

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
        $activeSmoke = $smokeRows[2]

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
    @{exit_code=$exitCode} | ConvertTo-Json | Set-Content -Encoding UTF8 (Join-Path $OutputDir 'desktop-exit.json')
}
exit $exitCode
