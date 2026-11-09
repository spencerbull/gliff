# Windows acceptance evidence

Status: **in progress**. The Windows application and installer are not yet
declared ready. Test IDs and pass conditions were written before implementation
in [windows-plan.md](windows-plan.md); this report records actual evidence.

## Environment and build identity

- Implementation revision under test: `cee086156e7475d2e5ade20dfc2dbbf0081a44d1`.
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
| W01 | Pass for cee0861 | Native MSVC build, Clippy, all portable tests, import/dependency checks and MakeAppx package validation passed; ZIP/MSIX/test-signed artifacts downloaded from PR run 42638509177 and checksums verified. |
| W02 | Failed first GUI launch; fix under test | Clean VM `--version` passed with no development tools. GUI launch failed because eframe disables the Direct3D backend by default. Added an explicit Windows dx12 feature and native regression guard; new artifact/runtime retest required. |
| W03 | Pending | SSH authentication works; real Gliff video and 60-second recording still required. |
| W04 | Partial | Input/release/shortcut regressions pass; real Windows input flow pending. |
| W05 | Partial | Geometry/DPI and reconnect epoch tests pass; real resize/cursor/DPI evidence pending. |
| W06 | Partial | Persisted tabs and bounded reconnects tested in Rust; real UI lifecycle pending. |
| W07 | Partial | Real protocol Unicode transfer, ownership/cancellation/generation tests pass; Windows-to-Omarchy clipboard flow pending. |
| W08 | Partial | SSH quoting, protocol failures, deadlines and teardown tests pass; Windows runtime error/process checks pending. |
| W09 | Pending | SDK MSIX generation and signed install/upgrade fixtures are in native CI; installation/alias/upgrade/uninstall not yet exercised. |
| W10 | Partial | Branch and PR runs started; generator fixtures pass native WinGet semantic validation. Native distributions/checksums still required. |
| W11 | Pass through cee0861 | `scripts/check.sh` passed fmt, Clippy with warnings denied and all 305 Linux workspace tests. Follow-up paste modifier fixes and renderer feature change require another check. |

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
