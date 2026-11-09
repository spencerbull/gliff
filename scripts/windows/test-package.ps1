[CmdletBinding()]
param([Parameter(Mandatory)][string]$PortableZip, [Parameter(Mandatory)][string]$Msix,
    [string]$Publisher = 'CN=Gliff Windows Test')
. "$PSScriptRoot/common.ps1"
Assert-Windows
Test-PortableZip $PortableZip
$stage = Join-Path ([IO.Path]::GetTempPath()) "gliff-validate-$([guid]::NewGuid())"
try {
    # SDK unpack also checks the package; never use /nv to bypass validation.
    Invoke-Checked (Get-SdkTool makeappx.exe) @('unpack', '/v', '/p', (Resolve-Path $Msix).Path, '/d', $stage)
    $xml = Test-AppManifest (Join-Path $stage 'AppxManifest.xml') (Get-ReleaseVersion) $Publisher
    $expected = @('gliff.exe','LICENSE','NOTICE','README.txt','SHA256SUMS.txt','AppxManifest.xml',
        'Assets/StoreLogo.png','Assets/Square44x44Logo.png','Assets/Square150x150Logo.png')
    foreach ($name in $expected) { if (-not (Test-Path (Join-Path $stage $name) -PathType Leaf)) { throw "Missing MSIX entry $name" } }
    foreach ($file in Get-ChildItem $stage -File -Recurse) {
        $relative = [IO.Path]::GetRelativePath($stage, $file.FullName).Replace('\', '/')
        if ($relative -notin ($expected + @('AppxBlockMap.xml','[Content_Types].xml','AppxSignature.p7x','AppxMetadata/CodeIntegrity.cat'))) { throw "Unexpected MSIX entry $relative" }
    }
    foreach ($asset in @(@('StoreLogo',50), @('Square44x44Logo',44), @('Square150x150Logo',150))) {
        $bytes = [IO.File]::ReadAllBytes((Join-Path $stage "Assets/$($asset[0]).png"))
        if ($bytes.Length -lt 24 -or [Convert]::ToHexString($bytes[0..7]) -cne '89504E470D0A1A0A') { throw 'Invalid PNG signature.' }
        foreach ($offset in @(16,20)) {
            $dimension = [Net.IPAddress]::NetworkToHostOrder([BitConverter]::ToInt32($bytes, $offset))
            if ($dimension -ne $asset[1]) { throw "Incorrect asset dimensions: $($asset[0])" }
        }
    }
    # The ZIP is the tested executable source of truth for both distributions.
    $zip = [IO.Compression.ZipFile]::OpenRead((Resolve-Path $PortableZip))
    try {
        $stream = $zip.GetEntry('gliff.exe').Open(); $sha = [Security.Cryptography.SHA256]::Create()
        try { $hash = [Convert]::ToHexString($sha.ComputeHash($stream)) }
        finally { $sha.Dispose(); $stream.Dispose() }
    } finally { $zip.Dispose() }
    if ($hash -cne (Get-FileHash (Join-Path $stage 'gliff.exe')).Hash) { throw 'MSIX and ZIP executables differ.' }
    & "$PSScriptRoot/test-runtime-deps.ps1" -Executable (Join-Path $stage 'gliff.exe')
    Invoke-Checked (Join-Path $stage 'gliff.exe') @('--version')
    Write-Host 'Portable/MSIX contents, hashes, identity, assets and PE imports passed. Installation was not tested.'
} finally {
    if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
}
