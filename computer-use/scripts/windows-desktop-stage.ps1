# High supervisor for exactly one probe or one non-replayed suite invocation.
# The caller owns its kill-on-close job and watches progress with a hard deadline.
param([Parameter(Mandatory=$true)][string]$RequestFile)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$request = Get-Content -Raw -LiteralPath $RequestFile | ConvertFrom-Json
$stageLog = $RequestFile.Replace('-request.json','-supervisor.log')
Start-Transcript -Path $stageLog -Force | Out-Null
$env:GCU_DESKTOP_STAGE_STATE = $request.StateFile
. (Join-Path $PSScriptRoot 'windows-desktop.ps1')
. (Join-Path $PSScriptRoot 'windows-desktop-launch.ps1')
$context = $null
$child = $null
$result = @{ok=$false;stage=$request.Stage;variant=$request.Variant.Name;win32_error=0;exit_hex='not_created'}
try {
    Write-DesktopProgress "STAGE ENTERED: $($request.Stage); compiling launcher"
    Add-Type -Path (@('windows-desktop-token.cs','windows-desktop-access.cs','windows-desktop-defaults.cs','windows-desktop-process.cs') |
        ForEach-Object { Join-Path $PSScriptRoot $_ })
    Write-DesktopProgress 'STAGE opening restricted context; owner/default DACL repair enabled by default'
    $context = [Gcu.DesktopLauncher]::Open($request.ExpectedSessionId, $request.Workspace)
    $variant = $request.Variant
    # Parent owns optional desktop ACL grant/restoration, surviving this job's kill.
    $variant.RepairDesktop = $false
    if ($request.Stage -ne 'suite') {
        $result = Invoke-RestrictedDesktopProbe $context $variant $request.Stage $request.Workspace $request.OutputDir $request.ExpectedSessionId `
            -TimeoutMs ([Math]::Min(40000, $request.TimeoutMs))
    } else {
        if ($variant.DiagnosticOnly -or -not $variant.LowerIntegrity -or -not $variant.RepairTokenDefaults) {
            throw 'diagnostic profile cannot launch real suite'
        }
        $context.Configure($true, $true, $variant.DuplicatePrimary)
        Write-DesktopProgress "SUITE token: $($context.TokenSummary)"
        $arguments = [string[]]@('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',
            (Join-Path $request.Workspace 'smoke-windows.ps1'),'-DesktopChild',
            '-TestExe',$request.TestExe,'-WorkerExe',$request.WorkerExe,'-FixtureExe',$request.FixtureExe,
            '-OutputDir',$request.OutputDir,'-Suite',$request.Suite,'-ElevatedPid',"$($request.ElevatedPid)",
            '-ExpectedSessionId',"$($request.ExpectedSessionId)")
        if ($request.PSObject.Properties['NativeDiagnostics'] -and $request.NativeDiagnostics) {
            $arguments += '-NativeDiagnostics'
        }
        Write-DesktopProgress 'SUITE CreateProcess; NonInteractive; no std-handle inheritance; no retry'
        $child = $context.Start("$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe",
            $arguments, $variant.Method, $false, $variant.NewConsole, $variant.UseShowWindow)
        Write-DesktopProgress "SUITE created: pid=$($child.ProcessId); waiting for process handle and done.json" -ChildPid $child.ProcessId
        $state = Wait-DesktopChild $child 'suite process' -TimeoutMs 710000
        $result.last = $state
        $result.exit_code = $state.exit_code
        $result.exit_hex = $state.exit_hex
        $done = Join-Path $request.OutputDir 'desktop-exit.json'
        if (-not (Test-Path $done)) { throw "child exited without done.json; last=$($state | ConvertTo-Json -Compress)" }
        $oracle = Get-Content -Raw $done | ConvertFrom-Json
        if ($state.exit_code -ne $oracle.exit_code) { throw "child exit mismatch: process=$($state.exit_hex); done=$($oracle.exit_code)" }
        if (-not (Test-DesktopTokenFacts $oracle.token_facts $request.ExpectedSessionId)) { throw 'suite done.json token rejected' }
        $result.ok = $state.exit_code -eq 0
    }
} catch {
    $result.error = $_.Exception.ToString()
    if ($null -ne $context) { $result.token = $context.TokenSummary }
    $exception = $_.Exception
    while ($null -ne $exception) {
        if ($exception -is [ComponentModel.Win32Exception]) { $result.win32_error = $exception.NativeErrorCode; break }
        $exception = $exception.InnerException
    }
    Write-DesktopProgress "STAGE ERROR: $($result.error)"
} finally {
    # Report before potentially blocked cleanup; caller still enforces deadline.
    Write-DesktopProgress "STAGE RESULT: $($result | ConvertTo-Json -Compress -Depth 10)"
    $result | ConvertTo-Json -Depth 10 | Set-Content -Encoding UTF8 $request.ResultFile
    try { if ($null -ne $child) { try { $child.Stop() } finally { $child.Dispose() } } }
    finally { if ($null -ne $context) { $context.Dispose() } }
    Stop-Transcript | Out-Null
}
