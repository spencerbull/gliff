[CmdletBinding()]
param([string]$ResultsDirectory = (Join-Path $PSScriptRoot '../../artifacts/windows-tests'))
. "$PSScriptRoot/common.ps1"
Assert-Windows
$hostInfo = & rustc -vV
if ($LASTEXITCODE -ne 0 -or $hostInfo -notcontains 'host: x86_64-pc-windows-msvc') { throw 'Use native Windows x64 MSVC Rust.' }
New-Item $ResultsDirectory -ItemType Directory -Force | Out-Null
$ResultsDirectory = [IO.Path]::GetFullPath($ResultsDirectory)

# Rust and cc-rs (OpenH264 C++) must agree on /MT. Explicit target flags also
# prevent host proc-macros from inheriting unsupported crt-static settings.
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = '-C target-feature=+crt-static'
$env:CFLAGS_x86_64_pc_windows_msvc = '/MT'
$env:CXXFLAGS_x86_64_pc_windows_msvc = '/MT'
$packages = @('-p', 'gliff-desktop', '-p', 'gliff-proto', '-p', 'gliff-transport', '-p', 'gliff-sw')
Push-Location $script:RepoRoot
try {
    Invoke-Checked cargo (@('fmt', '--check') + $packages) (Join-Path $ResultsDirectory 'fmt.txt')
    # Record only the Windows closure, including build dependencies. Fail if a
    # Linux graphics crate leaks in; other workspace members remain Linux-only.
    $tree = @(& cargo tree -p gliff-desktop --target $script:Target --locked --prefix none)
    if ($LASTEXITCODE -ne 0) { throw 'cargo tree failed.' }
    $tree | Set-Content (Join-Path $ResultsDirectory 'dependencies.txt')
    if ($tree -match '^(gtk4|libadwaita|wayland-client|gbm|gliff-va|gliff-vk|hypr-capture|hypr-input) v') { throw 'Linux dependencies in the Windows client closure.' }
    Invoke-Checked cargo (@('clippy', '--target', $script:Target, '--all-targets', '--locked') + $packages + @('--', '-D', 'warnings')) (Join-Path $ResultsDirectory 'clippy.txt')
    Invoke-Checked cargo (@('test', '--target', $script:Target, '--locked') + $packages) (Join-Path $ResultsDirectory 'tests.txt')
    Invoke-Checked cargo @('build', '-p', 'gliff-desktop', '--target', $script:Target, '--release', '--locked') (Join-Path $ResultsDirectory 'build.txt')
    $exe = Join-Path $script:RepoRoot "target/$script:Target/release/gliff-desktop.exe"
    Invoke-Checked $exe @('--version') (Join-Path $ResultsDirectory 'version.txt')
    & "$PSScriptRoot/test-runtime-deps.ps1" -Executable $exe -Log (Join-Path $ResultsDirectory 'imports.txt')
} finally { Pop-Location }
