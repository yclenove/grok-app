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
