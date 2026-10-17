# Invoked only by the Windows signing step. Never dump the environment/certificate.
$ErrorActionPreference = 'Stop'
$state = & target/debug/xtask.exe release --signing-status --target $env:GCU_TARGET
if ($LASTEXITCODE -ne 0) { throw 'Invalid signing configuration' }
if ($state -eq 'unsigned') { Write-Host 'Windows certificate absent: leaving executable unsigned.'; exit 0 }
$dir = Join-Path $env:RUNNER_TEMP ([guid]::NewGuid().ToString())
$certificates = @()
try {
    New-Item -ItemType Directory -Path $dir | Out-Null
    $pfx = Join-Path $dir 'sign.pfx'
    [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:WINDOWS_CERTIFICATE))
    $password = ConvertTo-SecureString $env:WINDOWS_CERTIFICATE_PASSWORD -AsPlainText -Force
    $certificates = @(Import-PfxCertificate -FilePath $pfx -CertStoreLocation Cert:\CurrentUser\My -Password $password)
    $leaf = @($certificates | Where-Object {
        $_.HasPrivateKey -and ($_.EnhancedKeyUsageList.ObjectId.Value -contains '1.3.6.1.5.5.7.3.3')
    })
    if ($leaf.Count -ne 1) { throw 'Expected one Code Signing certificate with private key' }
    $tool = Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\bin\*\x64\signtool.exe' | Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $tool) { throw 'Windows SDK signtool missing' }
    & $tool.FullName sign /sha1 $leaf[0].Thumbprint /fd SHA256 /tr https://timestamp.digicert.com /td SHA256 target/cu-stage/payload/gcu.exe *> (Join-Path $dir 'sign.log')
    if ($LASTEXITCODE -ne 0) { throw 'Authenticode signing failed' }
    & $tool.FullName verify /pa /all target/cu-stage/payload/gcu.exe *> (Join-Path $dir 'verify.log')
    if ($LASTEXITCODE -ne 0) { throw 'Authenticode verification failed' }
    $signed = Get-AuthenticodeSignature target/cu-stage/payload/gcu.exe
    if ($signed.Status -ne 'Valid' -or $signed.SignerCertificate.Thumbprint -ne $leaf[0].Thumbprint) { throw 'Unexpected Authenticode identity' }
} finally {
    foreach ($cert in $certificates) { Remove-Item "Cert:\CurrentUser\My\$($cert.Thumbprint)" -Force -ErrorAction SilentlyContinue }
    if (Test-Path $dir) { Remove-Item $dir -Recurse -Force }
}
