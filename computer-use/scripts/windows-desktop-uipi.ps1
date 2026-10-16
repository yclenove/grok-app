# Only the original High parent calls this; restricted desktop stages never do.
function Start-UipiFixture {
    param([string]$FixtureExe, [string]$Directory, [string]$OutputDir,
        $ParentFacts, [int]$ExpectedSessionId,
        [scriptblock]$Start = { param($executable,$workingDirectory)
            # No shell broker, restricted token, scheduled task, or child script.
            Start-Process -FilePath $executable -WorkingDirectory $workingDirectory -NoNewWindow -PassThru
        },
        [scriptblock]$ReadToken = { param($processId) [Gcu.DesktopLauncher]::ReadFixtureToken($processId) },
        [scriptblock]$Now = { [double][Diagnostics.Stopwatch]::GetTimestamp() * 1000 / [Diagnostics.Stopwatch]::Frequency },
        [scriptblock]$Tick = { Start-Sleep -Milliseconds 50 })
    $result = @{process=$null;pid=0;reason=''}
    $token = $null
    $parent = "pid=$PID; $(Format-DesktopTokenFacts $ParentFacts)"
    $savedState = $env:GCU_FIXTURE_STATE
    $savedActivation = $env:GCU_FIXTURE_NO_ACTIVATE
    try {
        if ($ParentFacts.IntegrityRid -lt 12288 -or $ExpectedSessionId -le 0 -or
            $ParentFacts.SessionId -ne $ExpectedSessionId) { throw 'High parent in expected interactive session required' }
        # Keep the executable outside PrepareWorkspace's inheritable Medium label.
        New-Item -ItemType Directory -Path $Directory | Out-Null
        $executable = Join-Path $Directory 'hi.exe'
        Copy-Item -LiteralPath $FixtureExe -Destination $executable
        $env:GCU_FIXTURE_STATE = Join-Path $OutputDir 'high-state.json'
        $env:GCU_FIXTURE_NO_ACTIVATE = '1'
        Write-DesktopProgress "HIGH FIXTURE START: executable=$executable; ordinary parent Start-Process; parent $parent"
        $result.process = & $Start $executable $Directory
        $token = & $ReadToken $result.process.Id
        Write-DesktopProgress "HIGH FIXTURE TOKEN: $token; parent $parent"
        if ($token.Pid -ne $result.process.Id -or $token.IntegrityRid -lt 12288 -or
            $token.SessionId -ne $ExpectedSessionId) { throw "elevated fixture token invalid: $token" }
        $null = Wait-Oracle 'elevated fixture window' {
            $result.process.Refresh()
            @{exited=$result.process.HasExited;hwnd=$result.process.MainWindowHandle.ToInt64();token="$token"}
        } { param($s) -not $s.exited -and $s.hwnd -ne 0 } -Now $Now -Tick $Tick
        $result.pid = $result.process.Id
    } catch {
        # A missing High control invalidates only W-UIPI, never the main suite.
        $result.reason = "High UIPI fixture unavailable: $($_.Exception.Message); parent $parent"
        Write-DesktopProgress "W-UIPI SKIP: $($result.reason)"
    } finally {
        $env:GCU_FIXTURE_STATE = $savedState
        $env:GCU_FIXTURE_NO_ACTIVATE = $savedActivation
    }
    @{pid=$result.pid;reason=$result.reason;token="$token";parent=$parent} | ConvertTo-Json -Depth 6 |
        Set-Content -Encoding UTF8 (Join-Path $OutputDir 'uipi-fixture.json')
    return $result
}
