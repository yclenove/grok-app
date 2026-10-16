# Independent suite/case boundaries: failures remain failures and never stop a sibling.
function Invoke-DesktopSuites {
    param([string]$Suite, [scriptblock]$Contract, [scriptblock]$Smoke)
    $errors = [Collections.Generic.List[string]]::new()
    if ($Suite -ne 'Smoke') {
        try { & $Contract | Out-Host }
        catch { $errors.Add("contract: $($_.Exception.Message)"); Write-DesktopProgress "CONTRACT FAILED: $($_.Exception.Message); smoke still runs" }
    }
    if ($Suite -ne 'Contract') {
        try { & $Smoke | Out-Host }
        catch { $errors.Add("smoke: $($_.Exception.Message)"); Write-DesktopProgress "SMOKE FAILED: $($_.Exception.Message)" }
    }
    return @{exit_code=[int]($errors.Count -gt 0);errors=$errors.ToArray()}
}

function Invoke-SmokeCase {
    param($Row, [scriptblock]$Run, [scriptblock]$Save)
    Write-DesktopProgress "SMOKE BEGIN: $($Row.app)"
    try {
        $result = & $Run
        if ($result.outcome -notin @('passed','skipped') -or -not $result.reason) { throw 'invalid smoke result' }
        $Row.outcome = $result.outcome
        $Row.reason = $result.reason
    } catch {
        $Row.outcome = 'failed'
        $Row.reason = $_.Exception.Message
    }
    $label = switch ($Row.outcome) { 'passed' { 'PASS' }; 'skipped' { 'SKIP' }; default { 'FAIL' } }
    Write-DesktopProgress "[SMOKE $($Row.app)] ${label}: $($Row.reason)"
    & $Save
}

function Start-SmokeApplication {
    param([string]$FilePath, [string]$Arguments = '',
        [scriptblock]$Start = { param($options) Start-Process @options })
    try {
        $options = @{FilePath=$FilePath;PassThru=$true;ErrorAction='Stop'}
        if ($Arguments) { $options.ArgumentList = $Arguments }
        $process = & $Start $options
        return @{process=$process;reason=''}
    } catch {
        # This is a measured system-app launch failure, before any driver action.
        # Unknown script/driver/oracle errors are handled as FAIL by Invoke-SmokeCase.
        $errorText = $_.Exception.ToString()
        $exception = $_.Exception
        $win32 = $null
        while ($null -ne $exception) {
            if ($exception -is [ComponentModel.Win32Exception]) { $win32 = $exception.NativeErrorCode; break }
            $exception = $exception.InnerException
        }
        # Only explicit native availability failures qualify. Parameter-binding
        # mistakes and other harness errors must fail, rather than become Skip.
        if ($win32 -notin @(2,3,5,126,740,1155)) { throw }
        return @{process=$null;reason="system app unavailable: Start-Process '$FilePath' failed; Win32=$win32; $errorText"}
    }
}

function Stop-SmokeApplication {
    param($Process)
    if ($null -ne $Process) {
        try { if (-not $Process.HasExited) { Stop-Process -Id $Process.Id -Force -ErrorAction Stop } }
        finally { $Process.Dispose() }
    }
}

function Invoke-NotepadSmoke {
    param([string]$TempDir)
    $command = Get-Command notepad.exe -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($null -eq $command) { return @{outcome='skipped';reason='system app absent: Get-Command notepad.exe -CommandType Application returned no executable'} }
    $file = Join-Path $TempDir 'note.txt'
    [IO.File]::WriteAllText($file,'',[Text.UTF8Encoding]::new($false))
    $launch = Start-SmokeApplication $command.Source ('"'+$file+'"')
    if ($null -eq $launch.process) { return @{outcome='skipped';reason=$launch.reason} }
    $notepad = $launch.process
    try {
        $null = Wait-Oracle 'Notepad window' {
            $notepad.Refresh()
            @{hwnd=$notepad.MainWindowHandle.ToInt64();exited=$notepad.HasExited}
        } { param($s) $s.hwnd -ne 0 }
        $env:GCU_SMOKE_PID = "$($notepad.Id)"
        Remove-Item Env:GCU_SMOKE_HWND -ErrorAction SilentlyContinue
        Invoke-NativeCase 'smoke::live_notepad_write_save' -TimeoutMs 90000
        $expected = 'GCU ' + [char]0x4e2d + [char]0x6587 + ' ' + [char]::ConvertFromUtf32(0x1f469) + [char]0x200d + [char]::ConvertFromUtf32(0x1f4bb)
        $null = Wait-Oracle 'Notepad saved exact Unicode bytes' { [IO.File]::ReadAllText($file) } { param($v) $v -ceq $expected }
        return @{outcome='passed';reason='saved file content exactly matches Unicode text'}
    } finally { Stop-SmokeApplication $notepad }
}

function Invoke-ExplorerSmoke {
    param([string]$TempDir)
    $command = Get-Command explorer.exe -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
    if ($null -eq $command) { return @{outcome='skipped';reason='system app absent: Get-Command explorer.exe -CommandType Application returned no executable'} }
    $source = New-Item -ItemType Directory -Path (Join-Path $TempDir 'a')
    $dest = New-Item -ItemType Directory -Path (Join-Path $TempDir 'b')
    $shell = New-Object -ComObject Shell.Application
    $prior = @($shell.Windows() | ForEach-Object { $_.HWND })
    $launch = Start-SmokeApplication $command.Source ('/n,"'+$source.FullName+'"')
    if ($null -eq $launch.process) { return @{outcome='skipped';reason=$launch.reason} }
    $explorer = $null
    try {
        $explorer = Wait-Oracle 'new Explorer folder window' {
            @($shell.Windows() | Where-Object { $_.HWND -notin $prior -and $_.Document.Folder.Self.Path -eq $source.FullName })
        } { param($ws) $null -ne $ws -and @($ws).Count -eq 1 }
        $explorer = @($explorer)[0]
        $env:GCU_SMOKE_HWND = "$($explorer.HWND)"
        $root = [Windows.Automation.AutomationElement]::FromHandle([IntPtr]$explorer.HWND)
        $env:GCU_SMOKE_PID = "$($root.Current.ProcessId)"
        $env:GCU_SMOKE_DEST = $dest.FullName
        Invoke-NativeCase 'smoke::live_explorer_navigate' -TimeoutMs 90000
        $null = Wait-Oracle 'Explorer exact destination' { $explorer.Document.Folder.Self.Path } { param($p) $p -eq $dest.FullName }
        return @{outcome='passed';reason='Shell.Application folder path exactly matches destination'}
    } finally {
        # Explorer may broker into an existing process. Close only new windows
        # for our disposable folders, including a window arriving near timeout.
        foreach ($window in @($shell.Windows())) {
            if ($window.HWND -notin $prior -and $window.Document.Folder.Self.Path -in @($source.FullName,$dest.FullName)) { $window.Quit() }
        }
        $launch.process.Dispose()
    }
}

function Invoke-SettingsSmoke {
    $protocol = 'Registry::HKEY_CLASSES_ROOT\ms-settings'
    if (-not (Test-Path -LiteralPath $protocol)) { return @{outcome='skipped';reason="system app protocol absent: Test-Path '$protocol' returned False; ms-settings: cannot be launched"} }
    $previous = @(Get-Process SystemSettings -ErrorAction SilentlyContinue | ForEach-Object { $_.Id })
    $launch = Start-SmokeApplication 'ms-settings:'
    if ($null -eq $launch.process) {
        # ShellExecute may legitimately return no process for a protocol. Only
        # an actual launch error qualifies for Skip; otherwise observe the UI.
        if ($launch.reason) { return @{outcome='skipped';reason=$launch.reason} }
    } else { $launch.process.Dispose() }
    $settingsProcess = $null
    try {
        $settingsProcess = Wait-Oracle 'Settings process' {
            @(Get-Process SystemSettings -ErrorAction SilentlyContinue | Where-Object { $_.Id -notin $previous })
        } { param($ps) $null -ne $ps -and @($ps).Count -eq 1 }
        $settingsProcess = @($settingsProcess)[0]
        $condition = [Windows.Automation.PropertyCondition]::new([Windows.Automation.AutomationElement]::ProcessIdProperty,$settingsProcess.Id)
        $root = Wait-Oracle 'Settings UIA window' {
            $windows = [Windows.Automation.AutomationElement]::RootElement.FindAll([Windows.Automation.TreeScope]::Children,[Windows.Automation.Condition]::TrueCondition)
            foreach ($window in $windows) {
                if ($window.Current.ProcessId -eq $settingsProcess.Id -or $null -ne $window.FindFirst([Windows.Automation.TreeScope]::Descendants,$condition)) { $window; break }
            }
        } { param($e) $null -ne $e }
        $before = @{}
        $initial = $root.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)
        foreach ($node in $initial) { $before[($node.GetRuntimeId() -join '.')] = $true }
        $env:GCU_SMOKE_HWND = "$($root.Current.NativeWindowHandle)"
        $env:GCU_SMOKE_PID = "$($settingsProcess.Id)"
        Invoke-NativeCase 'smoke::live_settings_search' -TimeoutMs 90000
        $null = Wait-Oracle 'Settings query and displayed results' {
            $nodes = $root.FindAll([Windows.Automation.TreeScope]::Descendants,[Windows.Automation.Condition]::TrueCondition)
            $query = $false
            $results = @()
            foreach ($node in $nodes) {
                $pattern = $null
                if ($node.TryGetCurrentPattern([Windows.Automation.ValuePattern]::Pattern,[ref]$pattern)) {
                    if ($pattern.Current.Value -ceq 'display') { $query = $true }
                }
                if (-not $node.Current.IsOffscreen -and $node.Current.ControlType -ne [Windows.Automation.ControlType]::Edit -and $node.Current.Name -match 'Display' -and -not $before.ContainsKey(($node.GetRuntimeId() -join '.'))) { $results += $node.Current.Name }
            }
            @{query=$query;results=$results}
        } { param($s) $s.query -and $s.results.Count -gt 0 }
        return @{outcome='passed';reason='independent UIA query and newly displayed search results'}
    } finally { Stop-SmokeApplication $settingsProcess }
}

function Invoke-WindowsSmoke {
    param($Rows, [string]$TempDir, [scriptblock]$Save)
    foreach ($row in $Rows) {
        Invoke-SmokeCase $row -Save $Save -Run {
            # Assembly/temp initialization is inside each case boundary too.
            Add-Type -AssemblyName UIAutomationClient
            Add-Type -AssemblyName UIAutomationTypes
            New-Item -ItemType Directory -Force -Path $TempDir | Out-Null
            switch ($row.app) {
                'Notepad' { Invoke-NotepadSmoke $TempDir }
                'Explorer' { Invoke-ExplorerSmoke $TempDir }
                'Settings' { Invoke-SettingsSmoke }
                default { throw "unknown smoke app: $($row.app)" }
            }
        }
    }
    if (@($Rows | Where-Object { $_.outcome -notin @('passed','skipped') }).Count -gt 0) { throw 'one or more system-app smoke cases failed; see per-app results' }
}
