# Dependency-free packaging/generator regression tests; runs on PowerShell 7
# on Linux as well as Windows. Network requests here are explicit fixture mocks.
[CmdletBinding()]
param([string]$ResultsDirectory)
. "$PSScriptRoot/common.ps1"
$stage = Join-Path ([IO.Path]::GetTempPath()) "gliff-script-tests-$([guid]::NewGuid())"
$script:Assertions = 0
function Assert-True([bool]$Condition, [string]$Message) {
    $script:Assertions++
    if (-not $Condition) { throw $Message }
}
function Assert-Throws([scriptblock]$Action, [string]$Pattern) {
    $script:Assertions++
    try { & $Action } catch {
        if ($_.Exception.Message -notmatch $Pattern) { throw "Wrong failure: $($_.Exception.Message); expected $Pattern" }
        return
    }
    throw "Expected failure: $Pattern"
}
try {
    New-Item (Join-Path $stage 'payload') -ItemType Directory -Force | Out-Null
    $payload = Join-Path $stage 'payload'
    foreach ($name in @('gliff.exe','LICENSE','NOTICE','README.txt')) { Set-Content (Join-Path $payload $name) "fixture $name" }
    Write-Hashes $payload @('gliff.exe','LICENSE','NOTICE','README.txt')
    $script:FixtureZip = Join-Path $stage 'fixture.zip'
    [IO.Compression.ZipFile]::CreateFromDirectory($payload, $script:FixtureZip)
    Test-PortableZip $script:FixtureZip
    $version = Get-ReleaseVersion
    $versionFixture = Join-Path $stage 'Cargo.toml'
    foreach ($badVersion in @('1.2.3-rc.1','1.2.3+build','65536.2.3','01.2.3')) {
        [IO.File]::WriteAllText($versionFixture, "[workspace.package]`nversion = `"$badVersion`"`n")
        Assert-Throws { Get-ReleaseVersion $versionFixture } 'stable numeric|65535'
    }
    [IO.File]::WriteAllText($versionFixture, "[workspace.package]`nversion = `"1.20.3`"`n[workspace.dependencies]`nversion = `"9.9.9`"`n")
    Assert-True ((Get-ReleaseVersion $versionFixture) -ceq '1.20.3') 'Version must come from workspace.package only.'
    $fixtureState = @{ Mode = 'ok'; Zip = $script:FixtureZip; Urls = [Collections.Generic.List[string]]::new() }
    # Scoped mock only: never used by the production generator entry point.
    function Invoke-WebRequest {
        param($Uri, $OutFile, $MaximumRedirection)
        $fixtureState.Urls.Add($Uri)
        if ($fixtureState.Mode -eq '404') { throw 'fixture HTTP 404' }
        if ($fixtureState.Mode -eq 'mismatch') { Set-Content $OutFile 'different public bytes' }
        else { Copy-Item $fixtureState.Zip $OutFile }
    }
    $output = Join-Path $stage 'manifests'
    & "$PSScriptRoot/new-winget-manifests.ps1" -PortableZip $script:FixtureZip -OutputDirectory $output
    $files = @(Get-ChildItem $output -Filter '*.yaml')
    Assert-True ($files.Count -eq 3) 'Expected winget multi-file manifests.'
    $installer = Get-Content (Join-Path $output 'SpencerBull.Gliff.installer.yaml') -Raw
    Assert-True ($installer.Contains('NestedInstallerType: portable') -and $installer.Contains('RelativeFilePath: gliff.exe')) 'Portable nested installer missing.'
    Assert-True ($installer.Contains((Get-FileHash $script:FixtureZip).Hash)) 'Must hash actual ZIP bytes.'
    Assert-True ($installer.Contains('ManifestVersion: 1.9.0') -and $installer.Contains('InstallerType: zip')) 'Incorrect installer schema metadata.'
    Assert-True (-not ($installer -match '(?m)^\s+Scope:')) 'Portable installers must leave scope to winget; a declared scope fails semantic validation.'
    Assert-True (-not $installer.Contains('InstallerType: msix')) 'Unsigned fixture must never yield an MSIX manifest.'
    Assert-True ($fixtureState.Urls[-1] -ceq "https://github.com/spencerbull/gliff/releases/download/v$version/gliff-$version-windows-x64.zip") 'Unexpected public release URL.'
    Assert-Throws { & "$PSScriptRoot/new-winget-manifests.ps1" -PortableZip $script:FixtureZip -OutputDirectory $output -Repository 'https://invalid' } 'owner/name'
    Assert-Throws { & "$PSScriptRoot/new-winget-manifests.ps1" -PortableZip $script:FixtureZip -OutputDirectory $output -PackageIdentifier "bad`nidentifier" } 'identifier'
    Assert-Throws { & "$PSScriptRoot/new-winget-manifests.ps1" -PortableZip $script:FixtureZip -OutputDirectory $output -Msix $script:FixtureZip -MsixPublisher 'CN=Gliff Windows Test' } 'native Windows|explicit trusted public Publisher'
    $fixtureState.Mode = '404'
    Assert-Throws { & "$PSScriptRoot/new-winget-manifests.ps1" -PortableZip $script:FixtureZip -OutputDirectory (Join-Path $stage 'missing') } '404'
    Assert-True (-not (Test-Path (Join-Path $stage 'missing'))) 'Failed public lookup wrote manifests.'
    $fixtureState.Mode = 'mismatch'
    Assert-Throws { & "$PSScriptRoot/new-winget-manifests.ps1" -PortableZip $script:FixtureZip -OutputDirectory $output } 'hash mismatch'
    Set-Content (Join-Path $payload 'gliff.exe') 'tampered'
    $badZip = Join-Path $stage 'tampered.zip'
    [IO.Compression.ZipFile]::CreateFromDirectory($payload, $badZip)
    Assert-Throws { Test-PortableZip $badZip } 'checksum mismatch'
    Set-Content (Join-Path $payload 'extra.dll') 'unwanted runtime'
    $extraZip = Join-Path $stage 'extra.zip'
    [IO.Compression.ZipFile]::CreateFromDirectory($payload, $extraZip)
    Assert-Throws { Test-PortableZip $extraZip } 'Unexpected portable ZIP entries'
    $template = Get-Content (Join-Path $script:RepoRoot 'packaging/windows/AppxManifest.xml.in') -Raw
    $manifest = Join-Path $stage 'AppxManifest.xml'
    $valid = $template.Replace('@VERSION@', "$version.0").Replace('@PUBLISHER@','CN=Gliff Windows Test')
    [IO.File]::WriteAllText($manifest, $valid)
    $null = Test-AppManifest $manifest $version 'CN=Gliff Windows Test'
    Assert-Throws { Test-AppManifest $manifest $version 'CN=Wrong Publisher' } 'identity mismatch'
    [IO.File]::WriteAllText($manifest, $valid.Replace('ProcessorArchitecture="x64"','ProcessorArchitecture="arm64"'))
    Assert-Throws { Test-AppManifest $manifest $version 'CN=Gliff Windows Test' } 'identity mismatch'
    [IO.File]::WriteAllText($manifest, $valid.Replace('Name="runFullTrust"','Name="broadFileSystemAccess"'))
    Assert-Throws { Test-AppManifest $manifest $version 'CN=Gliff Windows Test' } 'capability'
    [IO.File]::WriteAllText($manifest, $valid.Replace('Alias="gliff.exe"','Alias="other.exe"'))
    Assert-Throws { Test-AppManifest $manifest $version 'CN=Gliff Windows Test' } 'alias'
    [IO.File]::WriteAllText($manifest, $valid.Replace('BackgroundColor="transparent"','AppListEntry="none" BackgroundColor="transparent"'))
    Assert-Throws { Test-AppManifest $manifest $version 'CN=Gliff Windows Test' } 'Start menu'
    [IO.File]::WriteAllText($manifest, $valid.Replace('Name="Windows.Desktop"','Name="Windows.Universal"'))
    Assert-Throws { Test-AppManifest $manifest $version 'CN=Gliff Windows Test' } 'target family'
    if ($ResultsDirectory) {
        New-Item $ResultsDirectory -ItemType Directory -Force | Out-Null
        Copy-Item "$output/*.yaml" $ResultsDirectory -Force
        [IO.File]::WriteAllText((Join-Path ([IO.Path]::GetFullPath($ResultsDirectory)) 'FIXTURE-ONLY.txt'),
            'These are generator test fixtures using mocked public downloads, NOT valid submission manifests.')
        @{ assertions = $script:Assertions; status = 'PASS'; network = 'mocked fixtures'; platform = [Environment]::OSVersion.ToString() } |
            ConvertTo-Json | Set-Content (Join-Path $ResultsDirectory 'results.json')
    }
    Write-Host "PASS: $script:Assertions packaging/generator assertions (fixture network only)."
} finally { if (Test-Path $stage) { Remove-Item $stage -Recurse -Force } }
