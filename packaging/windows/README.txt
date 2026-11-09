Gliff for Windows x64

Extract the entire ZIP to a directory you own and run gliff.exe. No Rust,
Visual Studio, VC++ Redistributable, GTK, or OpenH264 DLL installation is
required. The MSVC CRT and OpenH264 are linked into the executable. Windows
10 2004 (build 19041) or Windows 11 x64 and a working graphics driver are
required. Initial release validation targets Windows 11.

Install the Windows OpenSSH Client optional feature if ssh.exe is absent.
Configure and test key/agent authentication with Windows OpenSSH first;
the remote Linux machine must have a compatible gliff-server and Hyprland.
SSH config, keys, agent and known-host checks are handled by OpenSSH.
Never disable host-key verification to work around a connection error.

Run gliff.exe --help for supported options. Windows-reserved shortcuts
(including Ctrl+Alt+Delete) cannot be captured. The initial keyboard scope
is US physical layout. See the project Windows documentation for current
functional limitations and validation status.

SHA256SUMS.txt inside the ZIP covers the executable and documentation.
The separate release SHA256SUMS-windows.txt covers distribution archives.
Compare hashes using Get-FileHash -Algorithm SHA256 before running files.

An MSIX file named TEST-ONLY is unsigned CI test material. It is not a
public installer. Do not disable Windows signing policy or trust a test
certificate on a real workstation. Public MSIX installation requires a
trusted publisher certificate and the publisher's documented release gate.

LICENSE and NOTICE include the Gliff and linked OpenH264 notices.
