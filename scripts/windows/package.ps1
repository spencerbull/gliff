[CmdletBinding()]
param(
    [string]$Executable = (Join-Path $PSScriptRoot '../../target/x86_64-pc-windows-msvc/release/gliff-desktop.exe'),
    [string]$OutputDirectory = (Join-Path $PSScriptRoot '../../artifacts/windows'),
    [string]$Publisher = 'CN=Gliff Windows Test'
)
. "$PSScriptRoot/common.ps1"
Assert-Windows
$version = Get-ReleaseVersion
if ([string]::IsNullOrWhiteSpace($Publisher)) { throw 'Publisher cannot be empty.' }
$Executable = (Resolve-Path $Executable).Path
& "$PSScriptRoot/test-runtime-deps.ps1" -Executable $Executable
New-Item $OutputDirectory -ItemType Directory -Force | Out-Null
$OutputDirectory = (Resolve-Path $OutputDirectory).Path
$stage = Join-Path ([IO.Path]::GetTempPath()) "gliff-package-$([guid]::NewGuid())"
$portable = Join-Path $stage 'portable'
$msix = Join-Path $stage 'msix'
$base = "gliff-$version-windows-x64"
try {
    New-Item $portable, $msix -ItemType Directory -Force | Out-Null
    Copy-Item $Executable (Join-Path $portable 'gliff.exe')
    Copy-Item (Join-Path $script:RepoRoot 'LICENSE'), (Join-Path $script:RepoRoot 'NOTICE') $portable
    Copy-Item (Join-Path $script:RepoRoot 'packaging/windows/README.txt') $portable
    Write-Hashes $portable @('gliff.exe', 'LICENSE', 'NOTICE', 'README.txt')
    $zip = Join-Path $OutputDirectory "$base.zip"
    if (Test-Path $zip) { Remove-Item $zip }
    [IO.Compression.ZipFile]::CreateFromDirectory($portable, $zip)
    Test-PortableZip $zip

    Copy-Item "$portable/*" $msix
    Copy-Item (Join-Path $script:RepoRoot 'packaging/windows/Assets') $msix -Recurse
    $template = Get-Content (Join-Path $script:RepoRoot 'packaging/windows/AppxManifest.xml.in') -Raw
    $manifest = $template.Replace('@PUBLISHER@', [Security.SecurityElement]::Escape($Publisher)).Replace('@VERSION@', "$version.0")
    [IO.File]::WriteAllText((Join-Path $msix 'AppxManifest.xml'), $manifest)
    $null = Test-AppManifest (Join-Path $msix 'AppxManifest.xml') $version $Publisher
    # /nv is deliberately absent: MakeAppx's default schema/semantic validation
    # must pass. There is no separate `makeappx validate` command in the SDK.
    Invoke-Checked (Get-SdkTool makeappx.exe) @('pack', '/v', '/o', '/d', $msix, '/p', (Join-Path $OutputDirectory "$base-TEST-ONLY.msix"))
    & "$PSScriptRoot/test-package.ps1" -PortableZip $zip -Msix (Join-Path $OutputDirectory "$base-TEST-ONLY.msix") -Publisher $Publisher
    Write-Hashes $OutputDirectory @("$base.zip", "$base-TEST-ONLY.msix") 'SHA256SUMS-windows.txt'
} finally {
    if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
}
