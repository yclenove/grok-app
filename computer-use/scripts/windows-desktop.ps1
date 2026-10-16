# Shared by the disposable desktop launcher and its portable self-tests.
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

function Get-DesktopTokenFacts {
    try { return [Gcu.DesktopLauncher]::ReadFacts() }
    catch {
        # Preserve independently readable facts even when querying the token fails.
        $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
        try {
            return @{ SessionId=[Diagnostics.Process]::GetCurrentProcess().SessionId
                IsAdmin=([Security.Principal.WindowsPrincipal]$identity).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
                IntegrityLevel="unavailable ($($_.Exception.Message))"; IntegrityRid=-1
                UserInteractive=[Environment]::UserInteractive; Desktop='unavailable'; TokenSummary="unavailable ($($_.Exception.Message))" }
        } finally { $identity.Dispose() }
    }
}

function Test-DesktopTokenFacts {
    param($Facts, [int]$ExpectedSessionId)
    return ($ExpectedSessionId -gt 0 -and $Facts.SessionId -eq $ExpectedSessionId -and
        -not $Facts.IsAdmin -and $Facts.IntegrityRid -eq 8192 -and
        $Facts.UserInteractive -and $Facts.Desktop -ieq 'WinSta0\Default')
}

function Format-DesktopTokenFacts {
    param($Facts)
    return "SessionId=$($Facts.SessionId); IsAdmin=$($Facts.IsAdmin); IntegrityLevel=$($Facts.IntegrityLevel); UserInteractive=$($Facts.UserInteractive); Desktop=$($Facts.Desktop)"
}

function Write-DesktopNotRun {
    param([string]$OutputDir, [string]$Suite, [string]$Reason)
    if ($Suite -ne 'Smoke') {
        $rows = [Collections.Generic.List[object]]::new()
        $ids = @(1..27 | ForEach-Object { 'C{0:00}' -f $_ }) + @('W-UIPI','W-HOTKEY')
        foreach ($id in $ids) { $rows.Add(@($id, @{outcome='not_run';reason=$Reason})) }
        @{passed=0;skipped=0;not_run=29;failed=0;rows=$rows.ToArray()} | ConvertTo-Json -Depth 8 |
            Set-Content -Encoding UTF8 (Join-Path $OutputDir 'contract.json')
        Write-Output "SUMMARY: 0 passed, 0 skipped, 29 not_run, 0 failed; $Reason"
    }
}
