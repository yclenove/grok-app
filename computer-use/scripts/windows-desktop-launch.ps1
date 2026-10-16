# Harmless native launch probes. The real suite is invoked once, after selection.
function Wait-DesktopChild {
    param($Child, [string]$Label, [int]$TimeoutMs = 30000,
        [scriptblock]$Now = { [double][Diagnostics.Stopwatch]::GetTimestamp() * 1000 / [Diagnostics.Stopwatch]::Frequency },
        [scriptblock]$Tick = { Start-Sleep -Milliseconds 50 },
        [scriptblock]$Observe = { param($s) })
    return Wait-Oracle $Label {
        $exited = $Child.HasExited
        $state = @{pid=$Child.ProcessId;exited=$exited;exit_code=$(if ($exited) { $Child.ExitCode } else { $null });
            exit_hex=$(if ($exited) { '0x{0:X8}' -f [uint32]$Child.ExitCode } else { 'pending' })}
        $null = & $Observe $state
        $state
    } { param($s) $s.exited } -TimeoutMs $TimeoutMs -Now $Now -Tick $Tick
}

function Get-DesktopLaunchVariants {
    param([switch]$DiagnoseTokens)
    # The confirmed DLL-init failure is optional and can never select a token.
    if ($DiagnoseTokens) {
        @{Name='AsUser/token-default-control/inherited-ACL/new-console';Method='AsUser';RepairDesktop=$false;
            NewConsole=$true;UseShowWindow=$true;RepairTokenDefaults=$false;LowerIntegrity=$true;DuplicatePrimary=$false;DiagnosticOnly=$true}
    }
    foreach ($repair in @($false,$true)) {
        # Hosted windows-2022 has verified this AsUser/no-window profile. Try it
        # before new consoles or desktop ACL changes, and stop once it works.
        foreach ($console in @($false,$true)) {
            foreach ($method in @('AsUser','WithToken')) {
                $suffix = if ($repair) { 'desktop-ACL' } else { 'inherited-ACL' }
                $consoleName = if ($console) { 'new-console' } else { 'no-window' }
                @{Name="$method/repaired-defaults/$suffix/$consoleName";Method=$method;RepairDesktop=$repair;
                    NewConsole=$console;UseShowWindow=$true;RepairTokenDefaults=$true;LowerIntegrity=$true;DuplicatePrimary=$false;DiagnosticOnly=$false}
            }
        }
    }
    foreach ($method in @('AsUser','WithToken')) {
        @{Name="$method/duplicate-primary/desktop-ACL/no-window";Method=$method;RepairDesktop=$true;
            NewConsole=$false;UseShowWindow=$true;RepairTokenDefaults=$true;LowerIntegrity=$true;DuplicatePrimary=$true;DiagnosticOnly=$false}
    }
    @{Name='AsUser/repaired-defaults/desktop-ACL/no-window/no-showwindow';Method='AsUser';RepairDesktop=$true;
        NewConsole=$false;UseShowWindow=$false;RepairTokenDefaults=$true;LowerIntegrity=$true;DuplicatePrimary=$false;DiagnosticOnly=$false}
    @{Name='AsUser/high-diagnostic/desktop-ACL/no-window';Method='AsUser';RepairDesktop=$true;
        NewConsole=$false;UseShowWindow=$true;RepairTokenDefaults=$true;LowerIntegrity=$false;DuplicatePrimary=$false;DiagnosticOnly=$true}
}

function Invoke-DesktopLaunchLadder {
    param([scriptblock]$Probe, [scriptblock]$Log = { param($line) Write-DesktopProgress $line },
        [switch]$DiagnoseTokens, [int]$TimeoutMs = 300000, [int]$StageTimeoutMs = 55000,
        [scriptblock]$Now = { [double][Diagnostics.Stopwatch]::GetTimestamp() * 1000 / [Diagnostics.Stopwatch]::Frequency })
    $attempts = [Collections.Generic.List[object]]::new()
    $start = & $Now
    foreach ($variant in Get-DesktopLaunchVariants -DiagnoseTokens:$DiagnoseTokens) {
        $ready = $true
        $stages = @('cmd-exit')
        if ($DiagnoseTokens) { $stages += 'cmd-whoami' }
        $stages += 'powershell'
        foreach ($stage in $stages) {
            $remaining = [int]($TimeoutMs - ((& $Now) - $start))
            if ($remaining -le 0) { throw "restricted launch ladder timeout; last attempts=$($attempts.ToArray() | ConvertTo-Json -Compress -Depth 8)" }
            $budget = [Math]::Min($StageTimeoutMs, $remaining)
            if ($stage -eq 'cmd-whoami') { $budget = [Math]::Min(5000, $budget) }
            $null = & $Log "DESKTOP BEGIN: variant=$($variant.Name); stage=$stage; timeout_ms=$budget"
            try { $result = & $Probe $variant $stage $budget }
            catch { $result = @{ok=$false;error=$_.Exception.Message} }
            if ($stage -eq 'powershell' -and $result.ok -and $result['script_entered'] -ne $true) {
                $result.ok = $false
                $result.error = 'PowerShell script entry oracle absent'
            }
            $entry = @{variant=$variant.Name;stage=$stage;result=$result}
            $attempts.Add($entry)
            $null = & $Log ("DESKTOP PROBE: " + ($entry | ConvertTo-Json -Compress -Depth 8))
            if (-not $result.ok -and $stage -ne 'cmd-whoami') { $ready = $false }
        }
        if ($variant.DiagnosticOnly) {
            $null = & $Log "DESKTOP DIAGNOSTIC: $($variant.Name); initialized_and_verified=$ready; eligible_for_suite=False"
        } elseif ($ready) {
            $null = & $Log "DESKTOP SELECTED: $($variant.Name); repaired owner/default DACL; explicit token environment; Unicode; WinSta0\Default; no inherited handles; Medium workspace"
            return $variant
        }
    }
    throw "all restricted launch variants failed; $($attempts.ToArray() | ConvertTo-Json -Compress -Depth 8)"
}

function Copy-DesktopPayload {
    param([string]$Workspace, [string]$TestExe, [string]$WorkerExe, [string]$FixtureExe)
    $bin = New-Item -ItemType Directory (Join-Path $Workspace 'bin')
    $out = New-Item -ItemType Directory (Join-Path $Workspace 'out')
    foreach ($name in @('smoke-windows.ps1','windows-desktop.ps1','windows-desktop-launch.ps1',
        'windows-desktop-token.cs','windows-desktop-access.cs','windows-desktop-defaults.cs',
        'windows-desktop-process.cs','windows-desktop-fixture.cs','windows-desktop-watch.ps1','windows-desktop-stage.ps1',
        'windows-desktop-uipi.ps1','windows-smoke.ps1')) {
        Copy-Item (Join-Path $PSScriptRoot $name) (Join-Path $Workspace $name)
    }
    $adversary = Join-Path (Split-Path $WorkerExe -Parent) 'gcu-contract-worker.exe'
    $sources = @($TestExe,$WorkerExe,$FixtureExe)
    if (Test-Path -LiteralPath $adversary) { $sources += $adversary }
    foreach ($source in $sources) {
        # No source ACLs or High integrity labels are copied to the fresh files.
        Copy-Item -LiteralPath $source -Destination (Join-Path $bin.FullName (Split-Path $source -Leaf))
    }
    foreach ($sourceDir in @((Split-Path $TestExe -Parent),(Split-Path $WorkerExe -Parent)) | Select-Object -Unique) {
        Get-ChildItem -LiteralPath $sourceDir -Filter '*.dll' | Copy-Item -Destination $bin.FullName -Force
    }
    return @{TestExe=(Join-Path $bin.FullName (Split-Path $TestExe -Leaf));
        WorkerExe=(Join-Path $bin.FullName (Split-Path $WorkerExe -Leaf));
        FixtureExe=(Join-Path $bin.FullName (Split-Path $FixtureExe -Leaf));OutputDir=$out.FullName;
        Script=(Join-Path $Workspace 'smoke-windows.ps1')}
}

function Copy-DesktopReports {
    param([string]$Source, [string]$Destination)
    # An empty directory is valid when creation failed before the first probe.
    Get-ChildItem -LiteralPath $Source -Force | Copy-Item -Destination $Destination -Recurse -Force
}

function Invoke-RestrictedDesktopProbe {
    param($Context, $Variant, [string]$Stage, [string]$Workspace, [string]$OutputDir, [int]$ExpectedSessionId,
        [int]$TimeoutMs = 40000)
    $prefix = $Variant.Name.Replace('/','-') + '-' + $Stage
    $identityFile = Join-Path $OutputDir ($prefix + '.json')
    $logFile = Join-Path $OutputDir ($prefix + '.log')
    $enteredFile = Join-Path $OutputDir ($prefix + '-entered.txt')
    $child = $null
    $state = $null
    $result = @{ok=$false;win32_error=0;exit_hex='not_created';token=$Context.TokenSummary;stage=$Stage;variant=$Variant.Name;
        options=$Variant;diagnostic_only=$Variant.DiagnosticOnly}
    try {
        Write-DesktopProgress "PROBE configuring token: $($Variant.Name)/$Stage"
        $Context.Configure($Variant.RepairTokenDefaults, $Variant.LowerIntegrity, $Variant.DuplicatePrimary)
        $result.token = $Context.TokenSummary
        Write-DesktopProgress "PROBE token: $($result.token)"
        if ($Stage -eq 'powershell') {
            # Encode only trusted harness paths. Files opened by the child prove
            # Medium write access as well as successful CLR/user32 initialization.
            $quote = { param($value) "'" + $value.Replace("'","''") + "'" }
            $access = & $quote (Join-Path $Workspace 'windows-desktop-access.cs')
            $token = & $quote (Join-Path $Workspace 'windows-desktop-token.cs')
            $defaults = & $quote (Join-Path $Workspace 'windows-desktop-defaults.cs')
            $process = & $quote (Join-Path $Workspace 'windows-desktop-process.cs')
            $helper = & $quote (Join-Path $Workspace 'windows-desktop.ps1')
            $lower = if ($Variant.LowerIntegrity) { '$true' } else { '$false' }
            $log = & $quote $logFile
            $oracle = & $quote $identityFile
            $entered = & $quote $enteredFile
            $program = @"
`$ErrorActionPreference = 'Stop'
try {
    [IO.File]::WriteAllText($entered, 'PowerShell script entered')
    Start-Transcript -Path $log -Force | Out-Null
    Add-Type -Path @($token,$access,$defaults,$process)
    . $helper
    `$facts = Get-DesktopTokenFacts
    Write-Host ("PROBE native token facts: " + `$facts.TokenSummary)
    [Console]::Out.Flush()
    `$facts | ConvertTo-Json -Depth 6 | Set-Content -Encoding UTF8 $oracle
    Write-Output (Format-DesktopTokenFacts `$facts)
    if (-not (Test-DesktopProbeFacts `$facts $ExpectedSessionId $lower)) { throw 'probe child token rejected' }
    Stop-Transcript | Out-Null
    exit 0
} catch {
    [IO.File]::AppendAllText($log, "`r`nPROBE ERROR: " + `$_)
    exit 1
}
"@
            $encoded = [Convert]::ToBase64String([Text.Encoding]::Unicode.GetBytes($program))
            Write-DesktopProgress 'PROBE CreateProcess: powershell (NoProfile/NonInteractive)'
            $child = $Context.Start("$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe",
                [string[]]@('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-EncodedCommand',$encoded), $Variant.Method, $Variant.RepairDesktop, $Variant.NewConsole, $Variant.UseShowWindow)
        } else {
            $command = if ($Stage -eq 'cmd-exit') { 'exit /b 0 > "' + $logFile + '" 2>&1' } else {
                '"' + $env:SystemRoot + '\System32\whoami.exe" /all > "' + $logFile + '" 2>&1'
            }
            Write-DesktopProgress "PROBE CreateProcess: $Stage"
            $child = $Context.StartCmd($command, $Variant.Method, $Variant.RepairDesktop, $Variant.NewConsole, $Variant.UseShowWindow)
        }
        Write-DesktopProgress "PROBE created: pid=$($child.ProcessId); waiting for process handle (no pipe EOF wait)" -ChildPid $child.ProcessId
        if ($Stage -eq 'cmd-whoami') { $TimeoutMs = [Math]::Min(5000, $TimeoutMs) }
        $state = Wait-DesktopChild $child "$($Variant.Name)/$Stage" -TimeoutMs $TimeoutMs
        $result.exit_code = $state.exit_code
        $result.exit_hex = $state.exit_hex
        $result.last = $state
        if ($state.exit_code -eq 0) {
            if ($Stage -eq 'cmd-exit') { $result.ok = $true }
            elseif ($Stage -eq 'cmd-whoami') {
                $result.last = if (Test-Path $logFile) { Get-Content -Raw $logFile } else { 'whoami output absent' }
                $integritySid = if ($Variant.LowerIntegrity) { 'S-1-16-8192' } else { 'S-1-16-12288' }
                $result.ok = (Test-Path $logFile) -and $result.last -match $integritySid -and $result.last -match 'SeChangeNotifyPrivilege'
            } else {
                $result.last = if (Test-Path $identityFile) { Get-Content -Raw $identityFile | ConvertFrom-Json } else { 'PowerShell token oracle absent' }
                $result.ok = (Test-Path $enteredFile) -and (Test-Path $identityFile) -and
                    (Test-DesktopProbeFacts $result.last $ExpectedSessionId $Variant.LowerIntegrity)
            }
        }
    } catch {
        $result.error = $_.Exception.ToString()
        $exception = $_.Exception
        while ($null -ne $exception) {
            if ($exception -is [ComponentModel.Win32Exception]) { $result.win32_error = $exception.NativeErrorCode; break }
            $exception = $exception.InnerException
        }
        $result.last = if ($null -ne $state) { $state } else { $_.Exception.Message }
    } finally {
        $result.token = $Context.TokenSummary
        if ($null -ne $child) {
            try {
                if (-not $child.HasExited) {
                    $child.Stop()
                    $null = Wait-DesktopChild $child 'probe termination' -TimeoutMs 10000
                }
                $result.exit_code = $child.ExitCode
                $result.exit_hex = '0x{0:X8}' -f [uint32]$child.ExitCode
            } finally { $child.Dispose() }
        }
        $result.desktop_security = $Context.DesktopSecurity
        $result.script_entered = Test-Path $enteredFile
        $result | ConvertTo-Json -Depth 10 | Set-Content -Encoding UTF8 (Join-Path $OutputDir ($prefix + '-result.json'))
        if (Test-Path $logFile) { Get-Content $logFile | Out-Host }
    }
    return $result
}
