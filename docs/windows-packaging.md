# Windows distribution

The Windows client is the separate `gliff-desktop` workspace package. Distribution
renames `gliff-desktop.exe` to `gliff.exe`; the Linux GTK client and source release
remain separate. This implementation follows [windows-plan.md](windows-plan.md).
It does not establish that W02–W09 runtime acceptance has passed.

## Build and validate

Use a native Windows x64 host, PowerShell 7.4+, stable MSVC Rust with rustfmt/clippy,
Visual Studio 2022 C++ build tools, NASM, and the Windows 10/11 SDK (MakeAppx and
SignTool). Run from an **x64 VS developer shell**, then start `pwsh`. The Windows
CI job configures this environment on `windows-2022`. NASM is only a build tool;
the ZIP does not require it, Rust, Visual Studio, or a redistributable installer.
Windows OpenSSH Client, a graphics driver, and Windows 10 build 19041 or newer
are runtime prerequisites. Initial runtime acceptance targets Windows 11.

```powershell
./scripts/windows/test-scripts.ps1 -ResultsDirectory artifacts/windows-tests/script-fixtures
./scripts/windows/build.ps1
./scripts/windows/package.ps1
```

`build.ps1` runs formatting, clippy with warnings denied, tests for
`gliff-desktop`, `gliff-proto`, `gliff-transport`, and `gliff-sw`, then a locked
release build and `--version`. It records the Windows dependency closure and
rejects known Linux graphics dependencies. It sets target-specific
`-C target-feature=+crt-static`, plus `/MT` C/C++ flags for cc-rs/OpenH264. A fresh
CI checkout avoids stale native object files; Cargo fingerprints these flags
when running locally. No root Cargo configuration is required. Keep these flags
when reproducing release binaries, including when using a custom target dir.

`test-runtime-deps.ps1` requires an x64 PE and inspects `dumpbin /dependents`,
including delayed imports. It rejects the VC runtime, UCRT imports, and DLLs
outside an explicit Windows system allowlist. If a new dependency appears,
investigate it; do not copy arbitrary DLLs from a developer workstation or relax
the check merely to make CI pass. This static check does not detect every
dynamic `LoadLibrary` call; clean VM testing is still required.

Output under `artifacts/windows/`:

| File | Contents/use |
| --- | --- |
| `gliff-VERSION-windows-x64.zip` | Root `gliff.exe`, `LICENSE`, `NOTICE`, `README.txt`, `SHA256SUMS.txt` |
| `gliff-VERSION-windows-x64-TEST-ONLY.msix` | Unsigned SDK package; disposable VM testing only |
| `SHA256SUMS-windows.txt` | SHA-256 hashes of those two archives |

The ZIP includes the source-build OpenH264 license in `NOTICE`. Internal hashes
cover the executable and all three accompanying documents. Hashes identify
bytes; they do not authenticate an unsigned download. The external checksum
file is regenerated after signing for public release artifacts.

Packaging reads only `[workspace.package].version`, maps `major.minor.patch` to
MSIX `major.minor.patch.0`, and fixes the supported architecture to x64/MSVC.
Components must be at most 65535. Prerelease/build suffixes are rejected rather
than silently mapped to colliding upgrade versions. Define a deliberate version
policy before packaging prereleases. ZIP timestamps and trusted signing
timestamps are not reproducible-build promises; scripts are repeatable and fail
on errors. They overwrite only their named generated artifacts and remove their
own temporary staging directories.

## MSIX identity, visuals, and validation

The manifest template is `packaging/windows/AppxManifest.xml.in`:

- Identity name: `SpencerBull.Gliff`; architecture: `x64`.
- Branch publisher: `CN=Gliff Windows Test`; no certificate is supplied.
- Start menu application `Gliff` launches `gliff.exe` as a full-trust desktop
  process. The `gliff.exe` app execution alias is declared; users/Windows can
  disable or override an alias independently of package installation.
- `runFullTrust` is the only capability. It is required for the ordinary desktop
  process and child OpenSSH process; it does not request elevation. No service,
  startup task, firewall rule, broad filesystem capability, or framework package
  dependency is declared.

`package.ps1` invokes SDK **MakeAppx pack with its default validation enabled**,
then `test-package.ps1` invokes SDK unpack with validation, checks manifest
identity/version/architecture/alias/capabilities, PNG signatures and dimensions,
expected entries, ZIP hashes, executable equality, PE imports, and `--version`.
There is no SDK `makeappx validate` command; `/nv` is never used. A signed package
additionally passes `signtool verify /pa /all /v`. These checks cover package
structure and signatures, not successful installation or GUI operation.

Committed 50×50, 44×44 and 150×150 PNG assets are rasterizations of the existing
`pkgbuild/gliff.svg`; regeneration needs only librsvg on Linux:

```sh
bash scripts/windows/render-assets.sh
```

The initial generation used rsvg-convert 2.63.4 with Cairo 1.18.6. Regenerating
with the same renderer and source produces the same bytes. Native CI consumes
the committed PNG files and needs no image tooling.

## CI and releases

`.github/workflows/ci.yml` runs on **all branch pushes**, PRs (including fork
PRs), `v*` tags and manual dispatch, including `spencerbull/gliff`'s
`windows-client` integration branch. The existing Linux fmt/clippy/test jobs and
tag version check remain in place. The Windows job runs the scripts above and
uploads `windows-x64` plus `windows-test-results` (logs, dependency/import
reports, fixture manifests marked `FIXTURE-ONLY`, and JSON test results). It
uses no signing secrets. The ordinary test MSIX is always named `TEST-ONLY`.

The Linux source-release job waits for **all Linux checks and Windows checks**.
Only it creates the GitHub release. The Windows attachment job waits for that
job, so it cannot race `gh release create`. It uploads an explicit allowlist:
portable ZIP, external checksums, and optionally a trusted signed MSIX. It
never uploads unsigned test MSIX, fixture winget files, signing material, or a
wildcard. It does not use `--clobber`; an existing asset causes failure rather
than replacing already distributed bytes. If a release was partly published,
inspect its assets and the workflow failure before recovery. A release-creation
failure or attachment failure is not automatically retried or hidden.

Configure the `windows-release` GitHub environment with required reviewers and
tag protections **before** enabling public signing. Protect tag creation and
the workflow itself. Environment access is the human release/signing gate; the
YAML cannot configure repository environment protection rules.

Signing inputs in that environment:

| Input | Kind | Meaning |
| --- | --- | --- |
| `GLIFF_WINDOWS_PUBLISHER` | Environment variable | Exact code-signing certificate subject, including DN components/order |
| `WINDOWS_SIGNING_PFX_BASE64` | Environment secret | Base64 PKCS#12 certificate/private key and chain |
| `WINDOWS_SIGNING_PFX_PASSWORD` | Environment secret | Nonempty PFX password |

With all inputs absent, tag releases publish the portable ZIP and its checksums
only. Partial configuration fails. With all present, CI repackages the tested
executable under that exact publisher, signs with SHA-256 and an RFC3161
timestamp, verifies SDK trust and the expected signer, validates contents again,
then attaches `gliff-VERSION-windows-x64.msix`. A self-signed/untrusted signer
fails this public path. The script loads the key ephemerally, writes a temporary
PFX, and deletes it in `finally`; it never imports certificates into a trust
store. Local invocation uses the same `GLIFF_SIGNING_PFX_BASE64` and
`GLIFF_SIGNING_PFX_PASSWORD` environment variables and an explicit `-Publisher`:

```powershell
./scripts/windows/package.ps1 -Publisher $env:GLIFF_WINDOWS_PUBLISHER
./scripts/windows/sign.ps1 -InputFile artifacts/windows/gliff-0.4.2-windows-x64-TEST-ONLY.msix `
  -OutputFile artifacts/windows/gliff-0.4.2-windows-x64.msix -Publisher $env:GLIFF_WINDOWS_PUBLISHER
```

Use the current workspace version in paths. `sign.ps1` requires a fresh output
file, preserves the unsigned input, and removes output it created if signing or
trust verification fails. Certificate rollover changes affecting package family
identity/upgrade behavior require an explicit migration decision. No public
publisher has been selected merely by committing this implementation.

## Winget preparation and submission gate

Do not submit before the artifacts are publicly downloadable and the clean VM
acceptance matrix has evidence. No sample placeholder manifests are checked in
as valid release manifests. After a real release:

```powershell
./scripts/windows/new-winget-manifests.ps1 `
  -PortableZip artifacts/windows-public/gliff-0.4.2-windows-x64.zip `
  -OutputDirectory artifacts/winget/SpencerBull.Gliff/0.4.2 -Validate
```

The generator defaults to `spencerbull/gliff` and `SpencerBull.Gliff`; a different
fork must pass `-Repository owner/repo` and review publisher metadata before
submission. It emits version, defaultLocale (`en-US`) and installer manifests
using manifest schema version **1.9.0**, each with a schema URL. The portable
installer uses `InstallerType: zip`, `NestedInstallerType: portable`, nested
`gliff.exe` and command alias `gliff` with user scope. The generator hashes the
actual local ZIP, downloads the canonical public GitHub release asset, and
requires the hashes to match. A missing URL, different bytes, malformed archive,
invalid repository/identifier or version fails before writing any manifests.
There is no skip-download flag. The default package identifier/metadata is a
fork proposal; public identifier ownership still needs maintainer agreement.

To add an MSIX entry to the same installer manifest, pass `-Msix PATH` and
`-MsixPublisher EXACT_SUBJECT`. This requires Windows, a trusted matching signer,
SDK signature validation, the same tested ZIP executable, and a matching public
MSIX download. Unsigned/test MSIX cannot enter generated submission manifests.
There is intentionally no MSIX manifest in the ordinary branch artifact.

`-Validate` requires the Windows Package Manager CLI and runs `winget validate
--manifest DIRECTORY`; generation without it is preparation, not a validation
claim. Then independently run Microsoft's current manifest validation and
`winget install --manifest DIRECTORY` in a disposable VM (enable local manifests
there if needed), exercise upgrades/uninstall, and inspect generated metadata.
The script neither installs nor submits. Public submission to
`microsoft/winget-pkgs` is an explicit maintainer gate, outside CI. No winget
credentials, submission automation, or publication action is added.

The dependency-free `test-scripts.ps1` runs on PowerShell 7.4+ on Windows or Linux.
It covers generator output/hash/URL handling, mocked missing/different public
downloads, malformed identifiers, tampered/extra ZIP files, rejected version
forms, manifest identity/architecture, unwanted capability and incorrect alias.
Its mocked download fixtures are clearly marked and must never be submitted.
Hosted CI exercises real packaging and SDK validation separately; generator
fixtures do not prove public release availability or winget CLI acceptance.

## Runtime acceptance remains separate

The parent validation stream owns isolated Windows VM testing and evidence:
clean extract/launch without developer tools; real SSH video/input/clipboard;
test-signed MSIX installation, Start menu, alias, upgrade and uninstall. See
W01–W11 in the plan. Test certificates may be created and trusted only inside
that disposable VM after taking a snapshot. No script here modifies host trust,
installs a package, relaxes policy, or claims these runtime tests passed. Keep
unsigned test artifacts out of public installers, and record precise blockers
if Windows, trusted signing, public URLs or VM access are unavailable.
