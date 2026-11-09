# Produce a disposable-VM installer fixture. Never used by public releases.
[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$UnsignedMsix,
    [string]$OutputDirectory = (Join-Path $PSScriptRoot '../../artifacts/windows-msix-test')
)
. "$PSScriptRoot/common.ps1"
Assert-Windows
$version = Get-ReleaseVersion
$base = "gliff-$version-windows-x64"
if (Test-Path $OutputDirectory) { throw 'Test fixture output must be a fresh directory.' }
$stage = Join-Path ([IO.Path]::GetTempPath()) "gliff-test-msix-$([guid]::NewGuid())"
$cert = $null
$complete = $false
New-Item $OutputDirectory -ItemType Directory -Force | Out-Null
try {
    $makeappx = Get-SdkTool makeappx.exe
    $signtool = Get-SdkTool signtool.exe
    Invoke-Checked $makeappx @('unpack', '/p', (Resolve-Path $UnsignedMsix).Path, '/d', $stage)
    $manifest = Join-Path $stage 'AppxManifest.xml'
    $xml = Test-AppManifest $manifest $version 'CN=Gliff Windows Test'
    $initial = Join-Path $OutputDirectory "$base-TEST-SIGNED.msix"
    $upgrade = Join-Path $OutputDirectory "$base-TEST-UPGRADE.msix"
    Copy-Item $UnsignedMsix $initial
    # Exercise a genuine MSIX upgrade without inventing another source release.
    $xml.Package.Identity.Version = "$version.1"
    $xml.Save($manifest)
    # Repacking must regenerate the package footprint for the changed manifest.
    foreach ($name in @('AppxBlockMap.xml', '[Content_Types].xml', 'AppxSignature.p7x', 'AppxMetadata/CodeIntegrity.cat')) {
        $path = Join-Path $stage $name
        if (Test-Path -LiteralPath $path) { Remove-Item -LiteralPath $path -Force }
    }
    Invoke-Checked $makeappx @('pack', '/v', '/d', $stage, '/p', $upgrade)
    $cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject 'CN=Gliff Windows Test' `
        -CertStoreLocation Cert:\CurrentUser\My -KeyExportPolicy NonExportable `
        -HashAlgorithm SHA256 -KeyUsage DigitalSignature -NotAfter (Get-Date).AddDays(30)
    Export-Certificate -Cert $cert -FilePath (Join-Path $OutputDirectory 'gliff-test.cer') | Out-Null
    foreach ($package in @($initial, $upgrade)) {
        Invoke-Checked $signtool @('sign', '/fd', 'SHA256', '/sha1', $cert.Thumbprint, '/s', 'My', $package)
        $signature = Get-AuthenticodeSignature $package
        if ($null -eq $signature.SignerCertificate -or $signature.SignerCertificate.Thumbprint -cne $cert.Thumbprint) {
            throw 'Test package does not contain the expected test signature.'
        }
    }
    @"
DISPOSABLE WINDOWS VM TEST FIXTURE ONLY

These packages use a self-signed, short-lived test certificate. Public release
jobs do not consume this artifact. The private signing key is not exported.
Certificate SHA1 thumbprint: $($cert.Thumbprint)
Certificate expiry (UTC): $($cert.NotAfter.ToUniversalTime().ToString('o'))

In an elevated PowerShell in your disposable test VM:
  Import-Certificate -FilePath .\gliff-test.cer -CertStoreLocation Cert:\LocalMachine\TrustedPeople
Then in the test user's PowerShell:
  Add-AppxPackage .\$base-TEST-SIGNED.msix
  Add-AppxPackage .\$base-TEST-UPGRADE.msix
The second package changes only the MSIX revision from $version.0 to $version.1.
Remove the test package with Get-AppxPackage SpencerBull.Gliff | Remove-AppxPackage.
Remove only the above test certificate from TrustedPeople after testing.
"@ | Set-Content (Join-Path $OutputDirectory 'README-TEST-ONLY.txt')
    Write-Hashes $OutputDirectory @("$base-TEST-SIGNED.msix", "$base-TEST-UPGRADE.msix", 'gliff-test.cer', 'README-TEST-ONLY.txt')
    $complete = $true
} finally {
    if ($null -ne $cert) { Remove-Item "Cert:\CurrentUser\My\$($cert.Thumbprint)" -DeleteKey }
    if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
    if (-not $complete -and (Test-Path $OutputDirectory)) { Remove-Item $OutputDirectory -Recurse -Force }
}
