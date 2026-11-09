[CmdletBinding()]
param([Parameter(Mandatory)][string]$Executable, [string]$Log)
. "$PSScriptRoot/common.ps1"
Assert-Windows
$output = @(& dumpbin.exe /nologo /headers /dependents $Executable)
if ($LASTEXITCODE -ne 0) { throw 'dumpbin failed (run from an x64 VS developer environment).' }
if ($Log) { $output | Set-Content $Log }
if (-not ($output -match '8664 machine \(x64\)')) { throw 'Executable is not x64.' }
$dlls = @($output | Where-Object { $_ -match '^\s+[\w.-]+\.dll\s*$' } | ForEach-Object { $_.Trim().ToLowerInvariant() } | Sort-Object -Unique)
if ($dlls.Count -eq 0) { throw 'No PE import dependencies found.' }
# All other dependencies require an explicit portability decision. Never copy
# arbitrary DLLs from a developer machine into the archive.
$system = @('kernel32.dll','user32.dll','gdi32.dll','gdi32full.dll','msimg32.dll','advapi32.dll','shell32.dll',
    'ole32.dll','oleaut32.dll','comdlg32.dll','comctl32.dll','ws2_32.dll','secur32.dll','bcrypt.dll','bcryptprimitives.dll','crypt32.dll',
    'cryptbase.dll','dwmapi.dll','dxgi.dll','d3d11.dll','d3d12.dll','d3dcompiler_47.dll','shcore.dll','shlwapi.dll',
    'imm32.dll','version.dll','uxtheme.dll','ntdll.dll','rpcrt4.dll','setupapi.dll','hid.dll','winmm.dll',
    'winhttp.dll','wininet.dll','wtsapi32.dll','userenv.dll','powrprof.dll','propsys.dll','avrt.dll','d2d1.dll',
    'dwrite.dll','opengl32.dll','psapi.dll','ncrypt.dll','iphlpapi.dll','dbghelp.dll','windowscodecs.dll')
foreach ($dll in $dlls) {
    if ($dll -match '^(vcruntime|msvcp|msvcr|ucrtbase|api-ms-win-crt)' -or
        ($dll -notin $system -and $dll -notmatch '^(api-ms-win-|ext-ms-win-)')) { throw "Unapproved external runtime dependency: $dll" }
}
Write-Host "x64 PE imports contain only approved Windows system DLLs: $($dlls -join ', ')"
