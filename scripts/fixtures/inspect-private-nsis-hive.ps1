param([string]$Hive, [Parameter(Mandatory)][string]$Key, [Parameter(Mandatory)][string]$Fence)
$ErrorActionPreference = 'Stop'
foreach ($view in @([Microsoft.Win32.RegistryView]::Registry32, [Microsoft.Win32.RegistryView]::Registry64)) {
  $hostRoot = [Microsoft.Win32.RegistryKey]::OpenBaseKey([Microsoft.Win32.RegistryHive]::CurrentUser, $view)
  try {
    foreach ($name in @($Key, $Fence)) {
      $hostKey = $hostRoot.OpenSubKey($name, $false)
      if ($null -ne $hostKey) { $hostKey.Dispose(); throw "Fixture key escaped to host $view" }
    }
  } finally { $hostRoot.Dispose() }
}
if (-not $Hive) { @{ hostFixtureKeysAbsent = $true } | ConvertTo-Json -Compress; exit 0 }
if (-not (Test-Path -LiteralPath $Hive -PathType Leaf)) { throw 'Private hive missing; refuse to create it during inspection' }
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class OwnedHiveReader {
  [DllImport("advapi32.dll", CharSet=CharSet.Unicode)]
  public static extern int RegLoadAppKeyW(string path, out IntPtr key, int access, int options, int reserved);
}
'@
$native = [IntPtr]::Zero
$code = [OwnedHiveReader]::RegLoadAppKeyW($Hive, [ref]$native, 0x20019, 1, 0)
if ($code -ne 0) { throw "Cannot read private hive: $code" }
$handle = [Microsoft.Win32.SafeHandles.SafeRegistryHandle]::new($native, $true)
$root = [Microsoft.Win32.RegistryKey]::FromHandle($handle, [Microsoft.Win32.RegistryView]::Registry64)
try {
  $base = $root.OpenSubKey('OwnedRoot\' + $Key + '\Uninstall', $false)
  if ($null -eq $base) { throw 'Registration missing from private hive' }
  try {
    $values = @{}
    foreach ($name in @('DisplayName','DisplayVersion','Publisher','MainBinaryName','InstallLocation','UninstallString')) {
      $values[$name] = $base.GetValue($name, $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames)
    }
    $product = $root.OpenSubKey('OwnedRoot\' + $Key + '\Product', $false)
    if ($null -eq $product) { throw 'Private installation root missing' }
    try { $location=$product.GetValue('', $null, [Microsoft.Win32.RegistryValueOptions]::DoNotExpandEnvironmentNames) }
    finally { $product.Dispose() }
    @{ hostFixtureKeysAbsent=$true; privateValues=$values; privateInstallRoot=$location; privateRegistryView=64 } | ConvertTo-Json -Compress
  } finally { $base.Dispose() }
} finally { $root.Dispose(); $handle.Dispose() }
