# Dot-sourced by test-windows-desktop.ps1; all measurements here are synthetic.
$smokeProtocol = { param($path) @{present=$true;url_protocol=$true;command='';delegate='{fixture}';handler_registered=$true} }
$smokeNoPackage = { param($name) @() }
$smokeNoFile = { param($path) $false }
$availability = Get-SmokeSettingsAvailability -SystemRoot '/fixture' -ReadProtocol $smokeProtocol -ReadPackages $smokeNoPackage -FileExists $smokeNoFile
Assert-True (-not $availability.available -and $availability.reason.Contains('windows.immersivecontrolpanel') -and
    $availability.reason.Contains('SystemSettings.exe') -and $availability.reason.Contains('"exists":false') -and
    $availability.reason.Contains('"packages":[]')) 'stale protocol with no Settings package/executable skips with all measured returns'
$availability = Get-SmokeSettingsAvailability -SystemRoot '/fixture' -ReadProtocol {
    param($path) @{present=$false;url_protocol=$false;command='';delegate='';handler_registered=$false}
} -ReadPackages $smokeNoPackage -FileExists { param($path) $true }
Assert-True (-not $availability.available -and $availability.reason.Contains('"present":false')) 'absent protocol is measured even if executable exists'
$availability = Get-SmokeSettingsAvailability -SystemRoot '/fixture' -ReadProtocol {
    param($path) @{present=$true;url_protocol=$true;command='';delegate='{missing}';handler_registered=$false}
} -ReadPackages $smokeNoPackage -FileExists { param($path) $true }
Assert-True (-not $availability.available -and $availability.reason.Contains('no registered shell handler or package')) 'stale protocol delegate is not an activation route even if executable exists'
$availability = Get-SmokeSettingsAvailability -SystemRoot '/fixture' -ReadProtocol {
    param($path) @{present=$true;url_protocol=$false;command='';delegate='';handler_registered=$false}
} -ReadPackages $smokeNoPackage -FileExists { param($path) $true }
Assert-True (-not $availability.available -and $availability.reason.Contains('"url_protocol":false')) 'protocol key without URL Protocol marker is not a URI registration'
$availability = Get-SmokeSettingsAvailability -SystemRoot '/fixture' -ReadProtocol $smokeProtocol -ReadPackages $smokeNoPackage -FileExists { param($path) $true }
Assert-True $availability.available 'existing system executable runs even without an Appx query result'
$availability = Get-SmokeSettingsAvailability -SystemRoot '/fixture' -ReadProtocol $smokeProtocol -ReadPackages {
    param($name) [pscustomobject]@{PackageFullName='fixture-package';InstallLocation='/fixture-package'}
} -FileExists { param($path) $path.Contains('fixture-package') }
Assert-True $availability.available 'package executable is checked before declaring Settings absent'
$probeFailure = ''
try {
    $null = Get-SmokeSettingsAvailability -SystemRoot '/fixture' -ReadProtocol $smokeProtocol -ReadPackages {
        param($name) throw 'package query denied'
    } -FileExists $smokeNoFile
} catch { $probeFailure = $_.Exception.Message }
Assert-True ($probeFailure.Contains('package query denied')) 'failed package query is not proof of absence'
$protocolLaunch = Start-SmokeApplication 'ms-settings:' -Protocol -Start {
    param($options)
    Assert-True (-not $options.ContainsKey('PassThru') -and $options.ErrorAction -eq 'Stop') 'URI activation requests no process handle and catches non-terminating launch errors'
}
Assert-True ($null -eq $protocolLaunch.process -and -not $protocolLaunch.reason) 'successful URI activation may return no process'

$smokeReport = Join-Path ([IO.Path]::GetTempPath()) ('smoke-selftest-' + [Guid]::NewGuid().ToString('N') + '.json')
$savedSmokeReport = $env:GCU_SMOKE_REPORT
try {
    $env:GCU_SMOKE_REPORT = 'outer-report'
    foreach ($outcome in @('passed','skipped','failed')) {
        $nativeFailure = ''
        $nativeResult = $null
        try {
            $nativeResult = Invoke-SmokeNativeCase 'smoke::fixture' $smokeReport -Native {
                param($name)
                @{test=$name;outcome=$outcome;reason='measured fixture reason'} | ConvertTo-Json | Set-Content -Encoding UTF8 $env:GCU_SMOKE_REPORT
                if ($outcome -eq 'failed') { throw 'exit 101' }
            }
        } catch { $nativeFailure = $_.Exception.Message }
        if ($outcome -eq 'failed') {
            Assert-True ($nativeFailure.Contains('measured fixture reason') -and $nativeFailure.Contains('exit 101')) 'explicit native failure preserves cause and exit'
        } else {
            Assert-True ($nativeResult.outcome -eq $outcome -and $nativeResult.reason -eq 'measured fixture reason') 'explicit native Skip/Pass reaches script oracle boundary'
        }
        Assert-True ($env:GCU_SMOKE_REPORT -eq 'outer-report') 'native report environment restored'
    }
    foreach ($fault in @('stale','wrong-test','failed-exit','invalid')) {
        @{test='smoke::fixture';outcome='skipped';reason='stale reason'} | ConvertTo-Json | Set-Content -Encoding UTF8 $smokeReport
        $nativeFailure = ''
        try {
            $null = Invoke-SmokeNativeCase 'smoke::fixture' $smokeReport -Native {
                param($name)
                if ($fault -eq 'stale') { return }
                $test = if ($fault -eq 'wrong-test') { 'wrong' } else { $name }
                $outcome = if ($fault -eq 'invalid') { 'not_run' } else { 'skipped' }
                @{test=$test;outcome=$outcome;reason='fixture reason'} | ConvertTo-Json | Set-Content -Encoding UTF8 $env:GCU_SMOKE_REPORT
                if ($fault -eq 'failed-exit') { throw 'exit 101' }
            }
        } catch { $nativeFailure = $_.Exception.Message }
        Assert-True ([bool]$nativeFailure) "stale/missing, wrong-test, bad exit and invalid reports cannot become Skip: $fault"
    }
} finally {
    $env:GCU_SMOKE_REPORT = $savedSmokeReport
    Remove-Item -LiteralPath $smokeReport -ErrorAction SilentlyContinue
}

# Cleanup is diagnostic only, including access denied and Dispose failures.
foreach ($targetRid in @(12288,8192)) {
    $cleanup = @{logs=[Collections.Generic.List[string]]::new();stops=0;disposed=0;reads=[Collections.Generic.List[int]]::new()}
    $process = [pscustomobject]@{Id=7;HasExited=$false;State=$cleanup}
    $process | Add-Member -MemberType ScriptMethod -Name Dispose -Value { $this.State.disposed++ }
    $row = @{app='Settings';outcome='not_run';reason='pending'}
    Invoke-SmokeCase $row -LogPrefix 'SELFTEST: ' -Save {} -Run {
        try { return @{outcome='skipped';reason='native measured refusal'} }
        finally {
            Stop-SmokeSettingsApplication $process -Stop {
                param($processId)
                $cleanup.stops++
                throw [ComponentModel.Win32Exception]::new(5)
            } -ReadToken {
                param($processId)
                $cleanup.reads.Add($processId)
                $rid = if ($processId -eq 7) { $targetRid } else { 8192 }
                [pscustomobject]@{IntegrityRid=$rid;IntegritySid="S-1-16-$rid";TokenElevation=[int]($rid -gt 8192)}
            } -Log { param($message) $cleanup.logs.Add($message) }
        }
    }
    Assert-True ($row.outcome -eq 'skipped' -and $row.reason -eq 'native measured refusal') 'cleanup denial cannot override the native outcome'
    Assert-True ($cleanup.stops -eq 1 -and $cleanup.disposed -eq 1 -and ($cleanup.reads -join ',') -eq "7,$PID") 'cleanup attempts stop once, measures both exact PIDs, and disposes'
    $log = $cleanup.logs -join ';'
    Assert-True ($log.Contains('cleanup skipped: access denied') -and $log.Contains('harness=Medium') -and
        $log.Contains('TokenElevation=') -and $log.Contains('S-1-16-8192')) 'cleanup denial is explicit with measured token facts'
    if ($targetRid -gt 8192) {
        Assert-True ($log.Contains('access denied (target integrity higher)') -and $log.Contains('target=High')) 'higher integrity cleanup denial is labelled only with proof'
    } else {
        Assert-True (-not $log.Contains('target integrity higher') -and $log.Contains('target=Medium')) 'same-level denial is logged without inventing a cause'
    }
}
$cleanup.logs.Clear()
$process | Add-Member -Force -MemberType ScriptMethod -Name Dispose -Value { throw 'fake Dispose failure' }
$row = @{app='Settings';outcome='not_run';reason='pending'}
Invoke-SmokeCase $row -LogPrefix 'SELFTEST: ' -Save {} -Run {
    try { throw 'original search assertion failed' }
    finally {
        Stop-SmokeSettingsApplication $process -Stop { param($processId) throw 'fake stop failure' } `
            -ReadToken { param($processId) throw 'fake token query failure' } -Log { param($message) $cleanup.logs.Add($message) }
    }
}
Assert-True ($row.outcome -eq 'failed' -and $row.reason -eq 'original search assertion failed') 'best-effort cleanup preserves the original assertion failure'
$log = $cleanup.logs -join ';'
Assert-True ($cleanup.logs.Count -eq 2 -and $log.Contains('fake stop failure') -and
    $log.Contains('fake token query failure') -and $log.Contains('fake Dispose failure') -and
    -not $log.Contains('target integrity higher')) 'unknown cleanup and measurement failures are all logged without guessed causes'
