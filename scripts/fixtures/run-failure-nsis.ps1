param(
  [Parameter(Mandatory)][string]$Binary,
  [Parameter(Mandatory)][string]$Owned,
  [Parameter(Mandatory)][ValidatePattern('^[a-z-]+$')][string]$Case,
  [Parameter(Mandatory)][ValidatePattern('^[a-zA-Z0-9-]+$')][string]$Nonce
)
$ErrorActionPreference = 'Stop'
$permit = Join-Path $Owned "permit-$Case"
$receipt = Join-Path $Owned "$Case.receipt"
if (Test-Path -LiteralPath $permit) { throw 'Refusing to reuse fixture permit' }
$p = [System.Diagnostics.Process]::new()
$p.StartInfo.FileName = $Binary
$p.StartInfo.WorkingDirectory = $Owned
$p.StartInfo.Arguments = '/CASE=' + $Case + ' /GROKUPDATEID=' + $Nonce
if ($Case -ne 'missing-receipt') { $p.StartInfo.Arguments += ' /GROKUPDATEFAILURE="' + $receipt + '"' }
$p.StartInfo.UseShellExecute = $false
$handle = [IntPtr]::Zero
try {
  if (-not $p.Start()) { throw 'Owned failure fixture failed to start' }
  $handle = $p.Handle
  $created = $p.StartTime.ToUniversalTime().ToFileTimeUtc().ToString([Globalization.CultureInfo]::InvariantCulture)
  $identity = $p.Id
  [System.IO.File]::WriteAllText($permit, 'owned test permit')
  if (-not $p.WaitForExit(15000)) {
    $p.Kill(); $p.WaitForExit()
    throw 'Owned failure callback exceeded its deadline'
  }
  $exited = $p.ExitTime.ToUniversalTime().ToFileTimeUtc().ToString([Globalization.CultureInfo]::InvariantCulture)
  @{ pid = $identity; created = $created; exited = $exited; exitCode = $p.ExitCode; handleHeld = ($handle -ne [IntPtr]::Zero) } | ConvertTo-Json -Compress
} finally {
  if ($handle -ne [IntPtr]::Zero -and -not $p.HasExited) { $p.Kill(); $p.WaitForExit() }
  $p.Dispose()
}
