# No Pester dependency: portable launcher policy/clock tests plus C# compilation.
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
. (Join-Path $PSScriptRoot 'windows-desktop.ps1')

function Assert-True([bool]$Value, [string]$Label) {
    if (-not $Value) { throw "assertion failed: $Label" }
}
foreach ($script in Get-ChildItem $PSScriptRoot -Filter '*windows*.ps1') {
    $tokens = $null
    $errors = $null
    $null = [Management.Automation.Language.Parser]::ParseFile($script.FullName, [ref]$tokens, [ref]$errors)
    Assert-True ($errors.Count -eq 0) "PowerShell syntax: $($script.Name): $errors"
}
Add-Type -Path (Join-Path $PSScriptRoot 'windows-desktop-token.cs')
[Gcu.DesktopLauncher]::CheckLayout()
Assert-True ([Gcu.DesktopLauncher]::QuoteArgument('a b') -ceq '"a b"') 'space quoting'
Assert-True ([Gcu.DesktopLauncher]::QuoteArgument('C:\a b\') -ceq '"C:\a b\\"') 'trailing slash quoting'
Assert-True ([Gcu.DesktopLauncher]::QuoteArgument('a"b') -ceq '"a\"b"') 'embedded quote escaping'
Assert-True ([Gcu.DesktopLauncher]::QuoteArgument('') -ceq '""') 'empty argument quoting'

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
Write-Output 'WINDOWS_DESKTOP_SELFTEST_OK (policy/ABI only; native token launch is checked by smoke-windows.ps1)'
