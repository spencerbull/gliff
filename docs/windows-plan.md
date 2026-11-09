# Windows client specification and validation plan

Status: agreed task scope translated into a concrete implementation plan,
written before implementation on 2026-11-09. Baseline: upstream Gliff 0.4.2.

## Architecture findings

The project is a Rust workspace. `gliff-server` captures Hyprland via Wayland,
injects input with virtual devices, and encodes H.264. `gliff-proto` defines
protocol v1 (CBOR framing and out-of-band payloads). `gliff-transport` owns SSH,
framing, and clipboard transfer machinery. `gliff-sw` provides the portable
OpenH264 CPU pipeline, including Single420 and Dual420 decoding. GTK4/libadwaita
in `crates/gliff` and VA-API/Vulkan DMA-BUF rendering are Linux-specific. The
existing CI checks Linux and creates source releases; it packages no Windows
or macOS application. No macOS frontend was found in the checkout or upstream
branch list.

Add a separate Rust desktop frontend in `crates/gliff-desktop` using
eframe/egui/winit for Windows windowing and rendering. Reuse `gliff-proto`,
`gliff-transport`, and `gliff-sw` directly. Keep protocol v1 and the remote
Linux session architecture. A runtime-discovered IPC compatibility fix also
accepts current Hyprland Lua rejection text for headless monitor mode changes. Isolate window/input/clipboard adapters from session
logic so another frontend can reuse the latter. Preserve the existing Linux
GTK frontend and its hardware acceleration. Windows initially decodes on the
CPU; hardware decode is a later, measured optimization, not a prerequisite.
The desktop executable is `gliff-desktop`; distribution renames it `gliff.exe`.

## Initiatives and deliverables

1. **Portable client.** Rust desktop UI; address entry and CLI host, multiple
   sessions/tabs, connect/disconnect/reconnect with bounded retries, useful
   status/errors, and Windows per-user persistence. Mirror selected/focused
   output or request a headless output; aspect-correct presentation, resize
   and DPI-aware pointer mapping, cursor, mouse buttons and wheel. US physical
   keyboard mapping with modifier forwarding and Shift+Escape release; release
   held keys/buttons on focus loss or disconnect. Document unsupported layout
   or reserved OS shortcut behavior honestly. Text clipboard in both
   directions with `--no-clipboard`; richer formats may remain a documented
   gap. SSH uses Windows OpenSSH config/agent/keys and known-host policy; give
   actionable errors and never disable host-key verification.
2. **Distribution.** Windows x64 MSVC build on Windows runners. Portable ZIP
   with executable, license/notice and prerequisites. SDK-generated MSIX with
   manifest, visual assets, Start menu entry and executable alias; validate
   contents, identity, architecture/version and package install behavior.
   Support optional trusted signing without embedding secrets. Branch pushes,
   PRs and manual runs must exercise Windows checks and upload test artifacts;
   tagged releases attach artifacts after checks. Generate winget multi-file
   manifests from actual artifact hashes/URLs and document validation and
   submission gates. Do not submit prematurely or invent hashes.
3. **Validation environment.** Prefer an isolated Windows 11 x64 VM on
   omaprox, provision only new resources, record CPU/RAM/disk and cleanup.
   Target 8 vCPU, 16 GiB RAM, 100 GiB disk if capacity permits. Use a separate
   test Hyprland session or authorized test host. A Windows CI build proves
   compilation and package structure; a real Windows desktop proves operation.
4. **Independent review.** Review protocol framing, untrusted sizes, process
   lifecycle, input cleanup, SSH security, signing, release gates and installers.
   Verify findings in source, fix them, then rerun affected tests.

## Known hurdles and choices

- Wayland/GTK/VA-API cannot be made portable by changing the CI target alone.
  A small separate frontend avoids invasive rewrites of the working Linux UI.
- OpenH264 needs a C++ toolchain; Windows CI must select MSVC and account for
  runtime DLL dependencies (prefer static CRT for a genuinely portable ZIP).
- Window libraries do not promise capture of Windows-reserved shortcuts or
  Secure Attention Sequence. Shift+Escape and focus loss must always release
  remote state; Ctrl+Alt+Delete cannot be forwarded as a local secure sequence.
- Windows layouts differ from evdev/XKB. First acceptance covers US layout;
  layout limitations must be visible in docs and never described as full parity.
- Windows SSH GUI authentication cannot silently hang waiting for a console
  password. Prefer key/agent authentication and bounded connection timeouts.
- MSIX needs a trusted certificate matching Publisher for ordinary installation.
  Unsigned CI output is test material; local test certificates must be explicitly
  trusted only in a disposable VM. Public signing identity and certificate
  remain a human release gate. Winget also needs stable public release URLs.
- Windows runtime access and installation media/licensing may require user
  input; keep building and testing independent pieces while access is pending.

## Acceptance tests defined before development

| ID | Requirement and observable pass condition | Evidence |
| --- | --- | --- |
| W01 | Native Windows x64 build, clippy and tests pass; no Linux libraries in the client dependency closure | CI logs and dependency inspection |
| W02 | Extract portable ZIP on a clean Windows 11 test VM; `gliff.exe --version` and GUI launch work without developer tools | Package hash, log, screenshot |
| W03 | Connect via SSH to a real Omarchy/Hyprland test session, view changing frames for at least 60 seconds | Screenshot and continuous video, host/build IDs |
| W04 | Remote terminal receives typed US text, modifiers, clicks, drag and scroll; release hotkey and alt-tab leave no stuck keys/buttons | Video plus remote observed output |
| W05 | Resize headless session; mirror letterboxing and 100%/150% DPI map clicks correctly; remote cursor visible | Screenshots/video and geometry log |
| W06 | Disconnect/reconnect and two tabs route input to the selected machine only; close/reopen restores configured hosts | Video and process/session logs |
| W07 | Unicode text clipboard works both directions; `--no-clipboard` prevents sharing | Video and assertion log |
| W08 | Unknown/changed host, missing SSH/server, incompatible protocol and interrupted connection produce bounded useful failures; no orphan SSH processes | Focused tests plus runtime log |
| W09 | MSIX passes MakeAppx validation; test-signed install, Start menu launch, alias, upgrade and uninstall work on disposable Windows | Manifest/package checks and screenshots/video |
| W10 | CI runs for the fork branch and PR; uploads ZIP/MSIX/checksums, winget manifest preparation rejects invalid inputs and uses real hashes | Workflow run and artifacts |
| W11 | Existing Linux fmt/clippy/tests pass; no unintended server/protocol behavior changes | `scripts/check.sh` log and independent review |

Evidence goes under a run-specific artifact directory with a README identifying
commit, Windows version, VM, remote server version, commands, test IDs, results,
and limitations. Never mark a row passed from mocks alone or fabricate runtime
media. Redact credentials and private clipboard contents from recordings.
If infrastructure blocks a test, record BLOCKED with the precise missing
resource; the overall Windows validation gate remains incomplete.

## Done criteria

The fork has a reviewable branch and draft PR, portable and MSIX CI artifacts,
winget preparation, passing build/regression/review checks, and genuine
Windows-to-Omarchy screenshot/video evidence for the acceptance matrix.
Trusted public signing and actual winget publication are documented human
gates. Any unmet acceptance item remains visibly open in TODO.md.
