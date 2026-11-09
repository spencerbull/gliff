#Requires -Version 7.4
# Shared by the Windows build and packaging entry points.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $false
$script:RepoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$script:Target = 'x86_64-pc-windows-msvc'

function Assert-Windows {
    if (-not $IsWindows) { throw 'This operation requires native Windows and PowerShell 7.4+.' }
}

function Invoke-Checked {
    param([string]$File, [string[]]$Arguments, [string]$Log)
    if ($Log) { & $File @Arguments 2>&1 | Tee-Object -FilePath $Log | Out-Host }
    else { & $File @Arguments | Out-Host }
    if ($LASTEXITCODE -ne 0) { throw "$File failed with exit code $LASTEXITCODE" }
}

function Get-ReleaseVersion {
    param([string]$ManifestPath = (Join-Path $script:RepoRoot 'Cargo.toml'))
    $toml = Get-Content $ManifestPath -Raw
    $section = [regex]::Match($toml, '(?ms)^\[workspace\.package\]\s*(.*?)(?=^\[|\z)').Groups[1].Value
    $version = [regex]::Match($section, '(?m)^version\s*=\s*"([^"]+)"').Groups[1].Value
    # Explicitly reject pre-release/build suffixes: stripping them causes MSIX upgrade collisions.
    if ($version -notmatch '^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$') {
        throw "Workspace version '$version' must be a stable numeric major.minor.patch for Windows packaging."
    }
    foreach ($part in $version.Split('.')) {
        if ([int64]$part -gt 65535) { throw 'MSIX version component exceeds 65535.' }
    }
    return $version
}

function Get-SdkTool {
    param([ValidateSet('makeappx.exe', 'signtool.exe')][string]$Name)
    Assert-Windows
    $sdk = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits/10/bin'
    $tools = @(Get-ChildItem $sdk -Directory | Where-Object Name -Match '^10\.\d+\.\d+\.\d+$' |
        Sort-Object { [version]$_.Name } -Descending)
    foreach ($dir in $tools) {
        $path = Join-Path $dir.FullName "x64/$Name"
        if (Test-Path $path) { return $path }
    }
    throw "Windows SDK x64 $Name not found. Install the Windows 10/11 SDK."
}

function Write-Hashes {
    param([string]$Directory, [string[]]$Names, [string]$FileName = 'SHA256SUMS.txt')
    $lines = foreach ($name in ($Names | Sort-Object)) {
        $hash = (Get-FileHash (Join-Path $Directory $name) -Algorithm SHA256).Hash.ToLowerInvariant()
        "$hash  $name"
    }
    [IO.File]::WriteAllText((Join-Path $Directory $FileName), (($lines -join "`n") + "`n"))
}

function Test-PortableZip {
    param([string]$Path)
    $zip = [IO.Compression.ZipFile]::OpenRead((Resolve-Path $Path))
    try {
        $expected = @('gliff.exe', 'LICENSE', 'NOTICE', 'README.txt', 'SHA256SUMS.txt')
        $actual = @($zip.Entries | ForEach-Object FullName)
        if (Compare-Object ($expected | Sort-Object) ($actual | Sort-Object)) { throw 'Unexpected portable ZIP entries.' }
        foreach ($entry in $zip.Entries) {
            if ($entry.Length -eq 0) { throw "Empty ZIP entry: $($entry.FullName)" }
        }
        $reader = [IO.StreamReader]::new($zip.GetEntry('SHA256SUMS.txt').Open())
        try { $hashLines = @($reader.ReadToEnd().TrimEnd().Split("`n")) } finally { $reader.Dispose() }
        if ($hashLines.Count -ne 4) { throw 'Portable checksum list must cover all four payload files.' }
        $seen = @{}
        foreach ($line in $hashLines) {
            if ($line.TrimEnd() -cnotmatch '^([a-f0-9]{64})  (gliff\.exe|LICENSE|NOTICE|README\.txt)$') { throw 'Invalid portable checksum entry.' }
            $hash = $Matches[1]; $name = $Matches[2]
            if ($seen.ContainsKey($name)) { throw 'Duplicate portable checksum entry.' }; $seen[$name] = $true
            $stream = $zip.GetEntry($name).Open()
            $sha = [Security.Cryptography.SHA256]::Create()
            try { $actualHash = [Convert]::ToHexString($sha.ComputeHash($stream)).ToLowerInvariant() }
            finally { $sha.Dispose(); $stream.Dispose() }
            if ($hash -cne $actualHash) { throw "Portable checksum mismatch: $name" }
        }
    } finally { $zip.Dispose() }
}

function Test-AppManifest {
    param([string]$Path, [string]$Version, [string]$Publisher)
    [xml]$xml = Get-Content $Path -Raw
    $ns = [Xml.XmlNamespaceManager]::new($xml.NameTable)
    $ns.AddNamespace('m', 'http://schemas.microsoft.com/appx/manifest/foundation/windows10')
    $ns.AddNamespace('uap', 'http://schemas.microsoft.com/appx/manifest/uap/windows10')
    $ns.AddNamespace('uap5', 'http://schemas.microsoft.com/appx/manifest/uap/windows10/5')
    $ns.AddNamespace('rescap', 'http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities')
    $identity = $xml.SelectSingleNode('/m:Package/m:Identity', $ns)
    if ($identity.Name -cne 'SpencerBull.Gliff' -or $identity.Version -cne "$Version.0" -or
        $identity.ProcessorArchitecture -cne 'x64' -or $identity.Publisher -cne $Publisher) { throw 'MSIX identity mismatch.' }
    $app = $xml.SelectSingleNode('/m:Package/m:Applications/m:Application', $ns)
    if ($app.Id -cne 'Gliff' -or $app.Executable -cne 'gliff.exe' -or $app.EntryPoint -cne 'Windows.FullTrustApplication') { throw 'Invalid MSIX application.' }
    $visual = $app.SelectSingleNode('uap:VisualElements', $ns)
    if ($visual.DisplayName -cne 'Gliff' -or $visual.Square150x150Logo -cne 'Assets\Square150x150Logo.png' -or
        $visual.Square44x44Logo -cne 'Assets\Square44x44Logo.png' -or $visual.HasAttribute('AppListEntry')) { throw 'Invalid Start menu visuals.' }
    $family = $xml.SelectSingleNode('/m:Package/m:Dependencies/m:TargetDeviceFamily', $ns)
    if ($family.Name -cne 'Windows.Desktop' -or $family.MinVersion -cne '10.0.19041.0') { throw 'Invalid Windows target family.' }
    $alias = $app.SelectSingleNode('m:Extensions/uap5:Extension/uap5:AppExecutionAlias/uap5:ExecutionAlias', $ns)
    if ($alias.Alias -cne 'gliff.exe') { throw 'Missing gliff.exe execution alias.' }
    $caps = @($xml.SelectNodes('/m:Package/m:Capabilities/*', $ns))
    if ($caps.Count -ne 1 -or $caps[0].LocalName -ne 'Capability' -or $caps[0].NamespaceURI -ne $ns.LookupNamespace('rescap') -or $caps[0].Name -ne 'runFullTrust') {
        throw 'Only the desktop runFullTrust capability is permitted.'
    }
    return $xml
}
