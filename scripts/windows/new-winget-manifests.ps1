[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$PortableZip,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [string]$Repository = 'spencerbull/gliff',
    [string]$PackageIdentifier = 'SpencerBull.Gliff',
    [string]$Msix,
    [string]$MsixPublisher,
    [switch]$Validate
)
. "$PSScriptRoot/common.ps1"
$version = Get-ReleaseVersion
if ($Repository -cnotmatch '^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$') { throw 'Repository must be owner/name.' }
if ($PackageIdentifier -cnotmatch '^[A-Za-z0-9]+(\.[A-Za-z0-9]+)+$') { throw 'Invalid winget package identifier.' }
if ($MsixPublisher -and -not $Msix) { throw 'MsixPublisher requires Msix.' }
Test-PortableZip $PortableZip

function Confirm-PublicArtifact {
    param([string]$LocalPath, [string]$FileName)
    $url = "https://github.com/$Repository/releases/download/v$version/$FileName"
    $temp = Join-Path ([IO.Path]::GetTempPath()) "gliff-public-$([guid]::NewGuid())"
    try {
        # No skip flag: generated manifests always reference public bytes that
        # were downloaded and matched to the locally inspected artifact.
        Invoke-WebRequest -Uri $url -OutFile $temp -MaximumRedirection 5
        $hash = (Get-FileHash $LocalPath -Algorithm SHA256).Hash
        if ($hash -cne (Get-FileHash $temp -Algorithm SHA256).Hash) { throw "Public hash mismatch: $url" }
        return @{ Url = $url; Hash = $hash }
    } finally { if (Test-Path $temp) { Remove-Item $temp -Force } }
}

$base = "gliff-$version-windows-x64"
$portable = Confirm-PublicArtifact $PortableZip "$base.zip"
$msixArtifact = $null
if ($Msix) {
    Assert-Windows
    if ([string]::IsNullOrWhiteSpace($MsixPublisher) -or $MsixPublisher -eq 'CN=Gliff Windows Test') { throw 'MSIX manifests require the explicit trusted public Publisher.' }
    $signature = Get-AuthenticodeSignature $Msix
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -cne $MsixPublisher) { throw 'winget MSIX must have a trusted matching signature.' }
    Invoke-Checked (Get-SdkTool signtool.exe) @('verify','/pa','/all','/v',(Resolve-Path $Msix).Path)
    & "$PSScriptRoot/test-package.ps1" -PortableZip $PortableZip -Msix $Msix -Publisher $MsixPublisher
    $msixArtifact = Confirm-PublicArtifact $Msix "$base.msix"
}
if ($Validate -and -not (Get-Command winget.exe -ErrorAction SilentlyContinue)) { throw 'winget.exe is required for -Validate.' }
New-Item $OutputDirectory -ItemType Directory -Force | Out-Null
function Yaml-Quote([string]$Value) { return "'" + $Value.Replace("'", "''") + "'" }
function Write-Manifest([string]$Suffix, [string]$Kind, [string]$Body) {
    $schema = "https://aka.ms/winget-manifest.$Kind.1.9.0.schema.json"
    $header = "# yaml-language-server: `$schema=$schema`nPackageIdentifier: $PackageIdentifier`nPackageVersion: $version`n"
    $footer = "`nManifestType: $Kind`nManifestVersion: 1.9.0`n"
    [IO.File]::WriteAllText((Join-Path $OutputDirectory "$PackageIdentifier$Suffix.yaml"), $header + $Body.Trim() + $footer)
}
Write-Manifest '' 'version' 'DefaultLocale: en-US'
$installer = @"
MinimumOSVersion: 10.0.19041.0
Commands:
- gliff
Installers:
- Architecture: x64
  InstallerType: zip
  NestedInstallerType: portable
  NestedInstallerFiles:
  - RelativeFilePath: gliff.exe
    PortableCommandAlias: gliff
  Scope: user
  InstallerUrl: $(Yaml-Quote $portable.Url)
  InstallerSha256: $($portable.Hash)
"@
if ($msixArtifact) {
    $installer += @"

- Architecture: x64
  InstallerType: msix
  Scope: user
  InstallerUrl: $(Yaml-Quote $msixArtifact.Url)
  InstallerSha256: $($msixArtifact.Hash)
"@
}
Write-Manifest '.installer' 'installer' $installer
Write-Manifest '.locale.en-US' 'defaultLocale' @"
PackageLocale: en-US
Publisher: Spencer Bull
PublisherUrl: $(Yaml-Quote "https://github.com/$($Repository.Split('/')[0])")
PackageName: Gliff
PackageUrl: $(Yaml-Quote "https://github.com/$Repository")
License: MIT
LicenseUrl: $(Yaml-Quote "https://github.com/$Repository/blob/v$version/LICENSE")
ShortDescription: Remote desktop client for Omarchy and Hyprland.
"@
if ($Validate) { Invoke-Checked winget.exe @('validate', '--manifest', (Resolve-Path $OutputDirectory).Path) }
Write-Host 'Prepared three winget manifests from matching public release bytes. No submission was made.'
