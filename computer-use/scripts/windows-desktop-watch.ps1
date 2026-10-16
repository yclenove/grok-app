# File-backed streams avoid inherited CI pipes and their unbounded EOF waits.
function Read-DesktopSharedText {
    param([string]$Path)
    if (-not (Test-Path -LiteralPath $Path)) { return '' }
    $stream = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::Read,
        ([IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete))
    $reader = [IO.StreamReader]::new($stream)
    try { return $reader.ReadToEnd() } finally { $reader.Dispose() }
}

function Get-DesktopProcessState {
    param([uint32]$ProcessId)
    if ($ProcessId -eq 0) { return @{pid=0;alive=$false;title='not created'} }
    $process = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if ($null -eq $process) { return @{pid=$ProcessId;alive=$false;title='exited'} }
    try { return @{pid=$ProcessId;alive=(-not $process.HasExited);title=$process.MainWindowTitle;session=$process.SessionId} }
    finally { $process.Dispose() }
}

function Wait-DesktopStage {
    param($Child, [string]$Label, [string]$OutputDir, [string]$StateFile, [int]$TimeoutMs,
        [scriptblock]$Now = { [double][Diagnostics.Stopwatch]::GetTimestamp() * 1000 / [Diagnostics.Stopwatch]::Frequency },
        [scriptblock]$Tick = { Start-Sleep -Milliseconds 200 },
        [scriptblock]$Log = { param($line) Write-DesktopProgress $line })
    $offsets = @{}
    $progress = @{next=0;last=$null}
    $start = & $Now
    $observe = {
        param($state)
        # Read snapshots of ordinary files, never ReadToEnd on a process pipe.
        foreach ($file in Get-ChildItem -LiteralPath $OutputDir -Filter '*.log' |
            Where-Object { $_.Name.StartsWith($Label) -or ($Label.EndsWith('-suite') -and $_.Name -eq 'desktop.log') }) {
            $content = Read-DesktopSharedText $file.FullName
            $offset = if ($offsets.ContainsKey($file.FullName)) { $offsets[$file.FullName] } else { 0 }
            if ($content.Length -lt $offset) { $offset = 0 }
            if ($content.Length -gt $offset) {
                $null = & $Log "DESKTOP STREAM [$($file.Name)]: $($content.Substring($offset))"
                $offsets[$file.FullName] = $content.Length
            }
        }
        $checkpoint = Read-DesktopSharedText $StateFile
        $nativePid = 0
        try {
            $parsed = $checkpoint | ConvertFrom-Json
            if ($null -ne $parsed) { $nativePid = [uint32]$parsed.child_pid }
        } catch { $checkpoint = "incomplete state JSON: $checkpoint" }
        $state.checkpoint = $checkpoint
        $state.child = Get-DesktopProcessState $nativePid
        $state.done = Test-Path (Join-Path $OutputDir 'desktop-exit.json')
        $state.report = Read-DesktopSharedText (Join-Path $OutputDir 'contract.json')
        $progress.last = $state
        $elapsed = (& $Now) - $start
        if ($elapsed -ge $progress.next -or $state.exited) {
            $null = & $Log "DESKTOP WAIT: $Label; elapsed_ms=$elapsed; last=$($state | ConvertTo-Json -Compress -Depth 8)"
            $progress.next = $elapsed + 10000
        }
    }
    try {
        return Wait-DesktopChild $Child $Label -TimeoutMs $TimeoutMs -Now $Now -Tick $Tick -Observe $observe
    } catch {
        # Emit before killing; dialogs and still-live processes are evidence.
        $null = & $Log "DESKTOP TIMEOUT/FAILURE: $Label; $($_.Exception.Message); last=$($progress.last | ConvertTo-Json -Compress -Depth 8)"
        throw
    }
}

function Invoke-SupervisedDesktopStage {
    param($Request, [string]$Workspace, [string]$OutputDir, [int]$TimeoutMs,
        [scriptblock]$Start = { param($arguments,$directory)
            [Gcu.DesktopLauncher]::StartSupervisor("$env:SystemRoot\System32\WindowsPowerShell\v1.0\powershell.exe", $arguments, $directory)
        },
        [scriptblock]$Now = { [double][Diagnostics.Stopwatch]::GetTimestamp() * 1000 / [Diagnostics.Stopwatch]::Frequency },
        [scriptblock]$Tick = { Start-Sleep -Milliseconds 200 })
    $prefix = $Request.Variant.Name.Replace('/','-') + '-' + $Request.Stage
    $requestFile = Join-Path $OutputDir ($prefix + '-request.json')
    $resultFile = Join-Path $OutputDir ($prefix + '-supervisor-result.json')
    $stateFile = Join-Path $OutputDir ($prefix + '-state.json')
    $Request.StateFile = $stateFile
    $Request.ResultFile = $resultFile
    $Request.TimeoutMs = $TimeoutMs
    $Request | ConvertTo-Json -Depth 10 | Set-Content -Encoding UTF8 $requestFile
    $child = $null
    $result = @{ok=$false;win32_error=0;exit_hex='not_created';stage=$Request.Stage;variant=$Request.Variant.Name}
    $started = & $Now
    try {
        $arguments = [string[]]@('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-File',
            (Join-Path $Workspace 'windows-desktop-stage.ps1'),'-RequestFile',$requestFile)
        $child = & $Start $arguments $Workspace
        Write-DesktopProgress "DESKTOP SUPERVISOR: $prefix; pid=$($child.ProcessId); timeout_ms=$TimeoutMs; kill-on-close process tree"
        $remaining = [Math]::Max(0, [int]($TimeoutMs - ((& $Now) - $started)))
        $state = Wait-DesktopStage $child $prefix $OutputDir $stateFile $remaining -Now $Now -Tick $Tick
        $result.last = $state
        $result.exit_hex = $state.exit_hex
        $result.exit_code = $state.exit_code
        if (-not (Test-Path $resultFile)) { throw "supervisor exited without result; last=$($state | ConvertTo-Json -Compress -Depth 8)" }
        # Keep a dictionary so adding diagnostics is safe under StrictMode even
        # for a minimal result produced before the native launcher initialized.
        $parsed = Read-DesktopSharedText $resultFile | ConvertFrom-Json
        $result = @{}
        foreach ($property in $parsed.PSObject.Properties) { $result[$property.Name] = $property.Value }
        if ($state.exit_code -ne 0) { throw "supervisor exit mismatch: $($state.exit_hex)" }
    } catch {
        $result = @{ok=$false;error=$_.Exception.ToString();last=(Read-DesktopSharedText $stateFile);
            stage=$Request.Stage;variant=$Request.Variant.Name;exit_hex='pending_or_not_created';win32_error=0}
        if ($null -ne $child) {
            $result.exit_hex = if ($child.HasExited) { '0x{0:X8}' -f [uint32]$child.ExitCode } else { 'pending' }
            $result.process = Get-DesktopProcessState $child.ProcessId
        }
        Write-DesktopProgress "DESKTOP STAGE FAILED: $($result | ConvertTo-Json -Compress -Depth 10)"
    } finally {
        if ($null -ne $child) {
            try { $child.Stop() } finally { $child.Dispose() }
            Write-DesktopProgress "DESKTOP TREE CLOSED: $prefix"
        }
        # Final file snapshots include stdout/stderr written at the deadline or
        # during termination; no dependence on the artifact upload succeeding.
        foreach ($file in Get-ChildItem -LiteralPath $OutputDir -Filter '*.log' |
            Where-Object { $_.Name.StartsWith($prefix) -or ($Request.Stage -eq 'suite' -and $_.Name -eq 'desktop.log') }) {
            Write-DesktopProgress "DESKTOP FINAL STREAM [$($file.Name)]: $(Read-DesktopSharedText $file.FullName)"
        }
        $result | ConvertTo-Json -Depth 10 | Set-Content -Encoding UTF8 (Join-Path $OutputDir ($prefix + '-watchdog-result.json'))
    }
    return $result
}
