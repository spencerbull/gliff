[CmdletBinding()]
param([Parameter(Mandatory)][string]$InputFile, [Parameter(Mandatory)][string]$OutputFile,
    [Parameter(Mandatory)][string]$Publisher)
. "$PSScriptRoot/common.ps1"
Assert-Windows
if ([string]::IsNullOrWhiteSpace($Publisher) -or $Publisher -eq 'CN=Gliff Windows Test') { throw 'An explicit public Publisher is required.' }
if (-not $env:GLIFF_SIGNING_PFX_BASE64 -or -not $env:GLIFF_SIGNING_PFX_PASSWORD) { throw 'Both signing secrets are required.' }
if ([IO.Path]::GetFullPath($InputFile) -eq [IO.Path]::GetFullPath($OutputFile)) { throw 'Signing must preserve the unsigned input.' }
$temp = Join-Path ([IO.Path]::GetTempPath()) "gliff-sign-$([guid]::NewGuid())"
New-Item $temp -ItemType Directory | Out-Null
$cert = $null
$ownsOutput = $false
try {
    $pfx = Join-Path $temp 'signing.pfx'
    [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:GLIFF_SIGNING_PFX_BASE64))
    $cert = [Security.Cryptography.X509Certificates.X509Certificate2]::new($pfx, $env:GLIFF_SIGNING_PFX_PASSWORD,
        [Security.Cryptography.X509Certificates.X509KeyStorageFlags]::EphemeralKeySet)
    if ($cert.Subject -cne $Publisher -or -not $cert.HasPrivateKey) { throw 'Signing certificate subject/private key does not match the explicit Publisher.' }
    $eku = @($cert.Extensions | Where-Object { $_ -is [Security.Cryptography.X509Certificates.X509EnhancedKeyUsageExtension] } |
        ForEach-Object { $_.EnhancedKeyUsages } | ForEach-Object Value)
    if ('1.3.6.1.5.5.7.3.3' -notin $eku) { throw 'Certificate must have the Code Signing EKU.' }
    Invoke-Checked (Get-SdkTool makeappx.exe) @('unpack','/p',(Resolve-Path $InputFile).Path,'/d',(Join-Path $temp 'unpacked'))
    $null = Test-AppManifest (Join-Path $temp 'unpacked/AppxManifest.xml') (Get-ReleaseVersion) $Publisher
    if (Test-Path $OutputFile) { throw 'Signing output already exists; choose a fresh output file.' }
    Copy-Item $InputFile $OutputFile -Force
    $ownsOutput = $true
    Invoke-Checked (Get-SdkTool signtool.exe) @('sign','/fd','SHA256','/f',$pfx,'/p',$env:GLIFF_SIGNING_PFX_PASSWORD,
        '/tr','https://timestamp.digicert.com','/td','SHA256',$OutputFile)
    Invoke-Checked (Get-SdkTool signtool.exe) @('verify','/pa','/all','/v',$OutputFile)
    $signature = Get-AuthenticodeSignature $OutputFile
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -cne $Publisher -or
        $signature.SignerCertificate.Thumbprint -cne $cert.Thumbprint) { throw 'Signed MSIX does not have a trusted matching signature.' }
} catch {
    if ($ownsOutput -and (Test-Path $OutputFile)) { Remove-Item $OutputFile -Force }
    throw
} finally {
    if ($null -ne $cert) { $cert.Dispose() }
    Remove-Item $temp -Recurse -Force
}
