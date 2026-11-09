# Windows acceptance evidence

Status: **in progress**. The Windows application and installer are not yet
declared ready. Test IDs and pass conditions were written before implementation
in [windows-plan.md](windows-plan.md); this report records actual evidence.

## Environment and build identity

- Implementation revision under test: `a097f812c1c0bad0655e008175948b15b3540d16`.
- Fork review: https://github.com/spencerbull/gliff/pull/1 (draft).
- Windows: Enterprise Evaluation 26H2, x64, 10.0.26300.9457, interactive test
  account `GLIFF-WINTEST\glifftest`, session 1.
- Isolated local Docker/KVM VM: 8 vCPU, 16 GiB RAM, 100 GB disk. Omaprox SSH
  access was unavailable. No Rust, Visual Studio or Windows SDK installed in
  the guest; QGA, Cua Driver and portable ffmpeg are test infrastructure.
- Server: installed Gliff 0.4.2 in a separate nested Hyprland session on the
  Omarchy host. SSH uses an ephemeral test key and an explicitly pinned host
  key. A local reverse relay connects the VM to this test-only SSH endpoint.
  User SSH settings and host firewall rules were not changed.

## Acceptance matrix

| ID | Current result | Evidence and remaining work |
| --- | --- | --- |
| W01 | Pass for a097f81 | Native MSVC build, Clippy, all portable tests, import/dependency checks and MakeAppx package validation passed; ZIP/MSIX/test-signed artifacts downloaded from PR run 42641834504 and checksums verified. |
| W02 | Pass for a097f81 | Clean VM version/hash and GUI launch passed with no developer tools. Explicit dx12 backend fixed the initial surface-creation failure. Real remote screenshot and build identity are below. |
| W03 | Partial | Real Windows SSH/H.264 session displays changing uptime. Recorded 110.73 seconds, but setup terminal obscured most of it; a continuous visible 60-second recording remains required. |
| W04 | Partial | Input/release/shortcut regressions pass; Windows remote log confirms mouse press/release/motion and typed `a`. Full keyboard/chord/drag/release matrix remains pending; window-scoped native keyboard adapter planned. |
| W05 | Partial | Geometry/DPI and reconnect epoch tests pass; real resize/cursor/DPI evidence pending. |
| W06 | Partial | Persisted tabs and bounded reconnects tested in Rust; real UI lifecycle pending. |
| W07 | Partial | Real protocol Unicode transfer, ownership/cancellation/generation tests pass; Windows-to-Omarchy clipboard flow pending. |
| W08 | Partial | SSH quoting, protocol failures, deadlines and teardown tests pass; Windows runtime error/process checks pending. |
| W09 | Pending | SDK MSIX generation and signed install/upgrade fixtures are in native CI; installation/alias/upgrade/uninstall not yet exercised. |
| W10 | Pass for branch artifacts | Branch and PR CI passed, ZIP/MSIX/test fixtures downloaded and checksums verified; native WinGet semantic validation passed. Public release hashes/URLs remain a release gate. |
| W11 | Pass through a097f81 | `scripts/check.sh` passed fmt, Clippy with warnings denied and all 307 Linux workspace tests. Independent findings reproduced and fixed; native keyboard adapter needs its own review/check cycle. |

The cee0861 portable executable SHA-256 is
`7cf58ecab9a420e48268bf164085c24bfee7a49db3d786b15243ccdb9c8a99a6`.
Its failed GUI launch reported
`Wgpu(CreateSurfaceError(...FailedToCreateSurfaceForAnyBackend({})))`.
Inspection of the resolved Windows features confirmed that eframe enabled
Metal/Vulkan but not Direct3D; the VM has no Vulkan driver. This is a client
build configuration bug, not an SSH or remote capture failure. No successful
remote-session evidence is claimed from this artifact.

## Native WinGet semantic validation

[Raw Windows result](evidence/windows/winget-validation.json), captured on
2026-11-09 using the installed WinGet 1.6.10121 in the interactive test account:

- Generated multi-file manifests with no portable `Scope`: validation exit 0,
  `Manifest validation succeeded.`
- Negative control with `Scope: user` added to that same portable installer:
  exit -1978335192 and `Scope is not supported for InstallerType portable.`

Only the YAML manifests were supplied to WinGet. The fixture-label TXT file
was first rejected as a manifest, so it was kept outside the validation input.
These manifests use fixture hashes and mocked release downloads. This proves
manifest syntax/semantics, not public artifact availability or an installable
release. The production generator still requires matching real public bytes.

Public signing identity, trusted release signing and actual winget submission
remain human release gates. No public release or submission has been made.

## First real Windows remote session

![Windows Gliff displaying and controlling the isolated Omarchy terminal](evidence/windows/a097f81-live-remote.png)

[Build hashes and actual remote input log](evidence/windows/a097f81-runtime.json)
identify the tested bytes. The clean VM successfully launched the portable
client, connected using its pinned-key SSH alias, decoded the changing CPU
H.264 stream, and delivered mouse events and an `a` keypress to the real
remote terminal. This establishes runtime progress; W03/W04 remain partial
until the full visible-video and input matrices pass.

The Unicode clipboard automation exposed a test broker bug: PowerShell's
redirected stdin used a legacy encoding. Cua exited on invalid UTF-8, ending
that recording. The broker now writes explicit UTF-8 bytes and reads request
JSON as UTF-8. Native Windows clipboard write/read preserves the exact Unicode
text; propagation to Omarchy has not yet passed. No product clipboard success
is inferred from that automation smoke check.
