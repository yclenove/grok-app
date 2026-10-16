# No Pester dependency: portable launcher policy/clock tests plus C# compilation.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'windows-desktop.ps1')
. (Join-Path $PSScriptRoot 'windows-desktop-launch.ps1')

function Assert-True([bool]$Value, [string]$Label) {
    if (-not $Value) { throw "assertion failed: $Label" }
}
foreach ($script in Get-ChildItem $PSScriptRoot -Filter '*windows*.ps1') {
    $tokens = $null
    $errors = $null
    $null = [Management.Automation.Language.Parser]::ParseFile($script.FullName, [ref]$tokens, [ref]$errors)
    Assert-True ($errors.Count -eq 0) "PowerShell syntax: $($script.Name): $errors"
}
$sources = @((Join-Path $PSScriptRoot 'windows-desktop-token.cs'), (Join-Path $PSScriptRoot 'windows-desktop-access.cs'))
if ($PSVersionTable.PSVersion.Major -ge 7) {
    # Linux Roslyn must also accept the language used by Windows PowerShell 5.1.
    Add-Type -Path $sources -CompilerOptions '/langversion:5'
} else { Add-Type -Path $sources }
[Gcu.DesktopLauncher]::CheckLayout()
Assert-True ([Gcu.DesktopLauncher]::QuoteArgument('a b') -ceq '"a b"') 'space quoting'
Assert-True ([Gcu.DesktopLauncher]::QuoteArgument('C:\a b\') -ceq '"C:\a b\\"') 'trailing slash quoting'
Assert-True ([Gcu.DesktopLauncher]::QuoteArgument('a"b') -ceq '"a\"b"') 'embedded quote escaping'
Assert-True ([Gcu.DesktopLauncher]::QuoteArgument('') -ceq '""') 'empty argument quoting'
Assert-True ([Gcu.DesktopLauncher]::CmdCommandLine('C:\Windows\cmd.exe', '"C:\Windows\whoami.exe" /all > "C:\temp space\id.log" 2>&1') -ceq
    '"C:\Windows\cmd.exe" /d /s /c ""C:\Windows\whoami.exe" /all > "C:\temp space\id.log" 2>&1"') 'cmd redirection uses cmd quoting, without CRT backslash escapes'
$inherited = [Collections.Generic.Dictionary[string,string]]::new([StringComparer]::OrdinalIgnoreCase)
$inherited.Add('Temp','high')
$inherited.Add('TMP','high')
$inherited.Add('SystemRoot','C:\Windows')
$inherited.Add('=C:','C:\Windows')
$environment = [Gcu.DesktopLauncher]::EnvironmentText($inherited, 'C:\medium space')
Assert-True ($environment.EndsWith("`0`0") -and -not $environment.Contains('high')) 'Unicode environment has double NUL and replaces both temp paths'
Assert-True ($environment.Contains("Temp=C:\medium space`0") -and $environment.Contains("TMP=C:\medium space`0") -and
    $environment.Contains("=C:=C:\Windows`0") -and $environment.Contains("SystemRoot=C:\Windows`0")) 'token environment preserves drive/system entries'
$sourceGroups = [Collections.Generic.Dictionary[string,uint32]]::new()
$sourceGroups.Add('S-1-5-32-544',7)
$sourceGroups.Add('S-1-5-5-1-2',[uint32]3221225479) # SE_GROUP_LOGON_ID | enabled | mandatory
$filteredGroups = [Collections.Generic.Dictionary[string,uint32]]::new($sourceGroups)
$filteredGroups['S-1-5-32-544'] = 16 # deny-only
[Gcu.DesktopLauncher]::CheckRestrictedGroups($sourceGroups,$filteredGroups)
foreach ($case in @('enabled-admin','missing-logon','denied-logon')) {
    $badGroups = [Collections.Generic.Dictionary[string,uint32]]::new($filteredGroups)
    if ($case -eq 'enabled-admin') { $badGroups['S-1-5-32-544'] = 7 }
    elseif ($case -eq 'missing-logon') { $null = $badGroups.Remove('S-1-5-5-1-2') }
    else { $badGroups['S-1-5-5-1-2'] = [uint32]3221225488 }
    $rejected = $false
    try { [Gcu.DesktopLauncher]::CheckRestrictedGroups($sourceGroups,$badGroups) } catch { $rejected = $true }
    Assert-True $rejected "reject restricted groups: $case"
}
$withoutLogon = [Collections.Generic.Dictionary[string,uint32]]::new()
$withoutLogon.Add('S-1-5-32-544',16)
[Gcu.DesktopLauncher]::CheckRestrictedGroups($withoutLogon,$withoutLogon)

# These checks also run under Windows PowerShell 5.1 on windows-2022.
# Reflection detects an accidentally ANSI STARTUPINFO desktop.
$launcher = [Gcu.DesktopLauncher]
$startup = $launcher.GetNestedType('StartupInfo', [Reflection.BindingFlags]::NonPublic)
Assert-True ($startup.StructLayoutAttribute.CharSet -eq [Runtime.InteropServices.CharSet]::Unicode) 'Unicode desktop name marshalling'
$tokenFacts = [Gcu.DesktopFacts]::new()
$tokenFacts.SessionId = 2
$tokenFacts.IntegrityLevel = 'Medium (S-1-16-8192)'
$roundTrip = @{exit_code=1;token_facts=$tokenFacts} | ConvertTo-Json -Depth 6 | ConvertFrom-Json
Assert-True ($roundTrip.token_facts.SessionId -eq 2 -and $roundTrip.token_facts.IntegrityLevel.Contains('8192')) 'done.json retains native token fields'

$facts = @{ SessionId=2; IsAdmin=$false; IntegrityLevel='Medium (S-1-16-8192)'; IntegrityRid=8192; UserInteractive=$true; Desktop='WinSta0\Default' }
Assert-True (Test-DesktopTokenFacts $facts 2) 'valid Medium interactive child'
foreach ($change in @(@{IsAdmin=$true}, @{IntegrityRid=12288}, @{IntegrityRid=4096},
    @{IntegrityRid=8448}, @{SessionId=0}, @{SessionId=3}, @{UserInteractive=$false}, @{Desktop='WinSta0\Winlogon'})) {
    $wrong = $facts.Clone()
    foreach ($key in $change.Keys) { $wrong[$key] = $change[$key] }
    Assert-True (-not (Test-DesktopTokenFacts $wrong 2)) "reject $($change | ConvertTo-Json -Compress)"
}
$diagnostic = Format-DesktopTokenFacts $facts
foreach ($key in @('SessionId=2','IsAdmin=False','IntegrityLevel=Medium','UserInteractive=True')) {
    Assert-True ($diagnostic.Contains($key)) "diagnostic includes $key"
}

$clock = @{ms=0;reads=0}
$null = Wait-Oracle 'ready' { $clock.reads++; @{ready=($clock.reads -eq 3)} } { param($s) $s.ready } `
    -Now { $clock.ms } -Tick { $clock.ms += 25 } -TimeoutMs 50
Assert-True ($clock.ms -eq 50 -and $clock.reads -eq 3) 'injected clock accepts boundary observation'
$failure = ''
try {
    $null = Wait-Oracle 'pending' { @{SessionId=3;ready=$false} } { param($s) $s.ready } `
        -Now { $clock.ms } -Tick { $clock.ms += 25 } -TimeoutMs 50
} catch { $failure = $_.Exception.Message }
Assert-True ($failure.Contains('timeout; last oracle=') -and $failure.Contains('"SessionId":3')) 'timeout retains last oracle'

$dir = Join-Path ([IO.Path]::GetTempPath()) ('wd' + [Guid]::NewGuid().ToString('N').Substring(0,6))
try {
    New-Item -ItemType Directory $dir | Out-Null
    Write-DesktopNotRun $dir All "token rejected; $diagnostic"
    $report = Get-Content -Raw (Join-Path $dir 'contract.json') | ConvertFrom-Json
    Assert-True ($report.passed -eq 0 -and $report.skipped -eq 0 -and $report.not_run -eq 29 -and $report.failed -eq 0) 'unavailable desktop is not_run'
    Assert-True ($report.rows.Count -eq 29) 'all native contract rows retained'
    foreach ($row in $report.rows) {
        Assert-True ($row[1].outcome -eq 'not_run' -and $row[1].reason.Contains($diagnostic)) 'each native row explains token rejection'
    }
} finally { Remove-Item -Recurse -Force $dir -ErrorAction SilentlyContinue }

# Probe policy never starts/replays the real suite. Every variant runs all three
# diagnostics, even after a DLL-init failure, and only a verified token can win.
$trace = [Collections.Generic.List[string]]::new()
$probe = {
    param($variant, $stage)
    $trace.Add("$($variant.Name)/$stage")
    $ok = $variant.Name -eq 'WithToken/desktop-ACL/no-window'
    @{ok=$ok;exit_code=$(if ($ok) { 0 } else { [uint32]3221225794 });win32_error=0;last='last token/oracle'}
}
$messages = [Collections.Generic.List[string]]::new()
$chosen = Invoke-DesktopLaunchLadder -Probe $probe -Log { param($line) $messages.Add($line) }
Assert-True ($chosen.Name -eq 'WithToken/desktop-ACL/no-window') 'fallback after post-create DLL initialization failure'
Assert-True ($trace.Count -eq 24) 'all stages executed for all eight variants'
Assert-True (($messages -join ' ').Contains('3221225794') -and ($messages -join ' ').Contains('last token/oracle')) 'failures preserve exit and last oracle'
$trace.Clear()
$chosen = Invoke-DesktopLaunchLadder -Probe { param($variant,$stage) $trace.Add($stage); @{ok=$true} } -Log { param($line) }
Assert-True ($chosen.Name -eq 'AsUser/inherited-ACL/new-console' -and $trace.Count -eq 3) 'stop diagnostics after the first fully verified variant'
$failure = ''
try {
    $null = Invoke-DesktopLaunchLadder -Probe { param($variant,$stage) throw "injected $stage Win32=5" } -Log { param($line) }
} catch { $failure = $_.Exception.Message }
Assert-True ($failure.Contains('all restricted launch variants failed') -and $failure.Contains('Win32=5') -and $failure.Contains('powershell')) 'all-failed not_run retains every stage error'

# Native handles are replaced by an oracle here; no cold-start timing dependency.
$clock.ms = 0
$fake = [pscustomobject]@{ProcessId=42;HasExited=$false;ExitCode=[uint32]3221225794}
$failure = ''
try {
    $null = Wait-DesktopChild $fake 'probe' -Now { $clock.ms } -Tick { $clock.ms += 10 } -TimeoutMs 20
} catch { $failure = $_.Exception.Message }
Assert-True ($failure.Contains('"pid":42') -and $failure.Contains('"exited":false')) 'probe timeout retains last process oracle'
$fake.HasExited = $true
$state = Wait-DesktopChild $fake 'probe' -Now { $clock.ms } -Tick { throw 'must not wait' }
Assert-True ($state.exit_hex -eq '0xC0000142' -and $state.exit_code -eq 3221225794) 'NTSTATUS is unsigned and hex in diagnostic'

$dir = Join-Path ([IO.Path]::GetTempPath()) ("wp '" + [Guid]::NewGuid().ToString('N').Substring(0,6))
try {
    New-Item -ItemType Directory $dir | Out-Null
    $variant = @{Name='AsUser/inherited-ACL/new-console';Method='AsUser';RepairDesktop=$false;NewConsole=$true}
    $mock = [pscustomobject]@{TokenSummary='SID=fixture; Integrity=Medium; SeChangeNotifyPrivilege=enabled';DesktopSecurity='D:fixture';
        OutputDir=$dir;Prefix=$variant.Name.Replace('/','-');Facts=$facts;EmitFacts=$false;FailCreate=$false}
    $mock | Add-Member ScriptMethod Start {
        param($exe,$arguments,$method,$repair,$console)
        if ($this.FailCreate) { throw [ComponentModel.Win32Exception]::new(5) }
        $program = [Text.Encoding]::Unicode.GetString([Convert]::FromBase64String($arguments[-1]))
        $tokens = $null; $errors = $null
        $null = [Management.Automation.Language.Parser]::ParseInput($program,[ref]$tokens,[ref]$errors)
        if ($errors.Count -ne 0) { throw "generated probe syntax: $errors" }
        if ($this.EmitFacts) {
            $this.Facts | ConvertTo-Json -Depth 6 | Set-Content -Encoding UTF8 (Join-Path $this.OutputDir ($this.Prefix + '-powershell.json'))
        }
        $process = [pscustomobject]@{ProcessId=42;HasExited=$true;ExitCode=[uint32]0}
        $process | Add-Member ScriptMethod Stop { throw 'already exited' }
        $process | Add-Member ScriptMethod Dispose { }
        return $process
    }
    $result = Invoke-RestrictedDesktopProbe $mock $variant powershell $dir $dir 2
    Assert-True (-not $result.ok -and $result.last -eq 'PowerShell token oracle absent') 'zero exit without external token oracle is rejected'
    $mock.EmitFacts = $true
    $result = Invoke-RestrictedDesktopProbe $mock $variant powershell $dir $dir 2
    Assert-True ($result.ok -and $result.last.SessionId -eq 2) 'external token oracle admits actual Medium facts'
    $mock.Facts = $facts.Clone(); $mock.Facts.IsAdmin = $true
    $result = Invoke-RestrictedDesktopProbe $mock $variant powershell $dir $dir 2
    Assert-True (-not $result.ok -and $result.last.IsAdmin) 'successful process cannot override rejected token oracle'
    $mock.FailCreate = $true
    $result = Invoke-RestrictedDesktopProbe $mock $variant powershell $dir $dir 2
    Assert-True (-not $result.ok -and $result.win32_error -eq 5 -and $result.token.Contains('SID=fixture')) 'creation failure retains Win32 error and token details'

    $source = New-Item -ItemType Directory (Join-Path $dir 'empty')
    $destination = New-Item -ItemType Directory (Join-Path $dir 'published')
    Copy-DesktopReports $source.FullName $destination.FullName
    Assert-True (@(Get-ChildItem $destination.FullName).Count -eq 0) 'empty probe output still allows not_run reporting'
    '{"exit_code":3221225794}' | Set-Content -Encoding UTF8 (Join-Path $source.FullName 'launch.json')
    Copy-DesktopReports $source.FullName $destination.FullName
    $published = Get-Content -Raw (Join-Path $destination.FullName 'launch.json') | ConvertFrom-Json
    Assert-True ($published.exit_code -eq 3221225794) 'probe artifacts return to the parent output directory'
} finally { Remove-Item -Recurse -Force $dir -ErrorAction SilentlyContinue }
Write-Output 'WINDOWS_DESKTOP_SELFTEST_OK (policy/ABI only; native token launch is checked by smoke-windows.ps1)'
