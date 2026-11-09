# Portable Gliff desktop client

`gliff-desktop` is the eframe/egui/winit frontend for Windows x64. Distribution
may rename `gliff-desktop.exe` to `gliff.exe`. The existing `gliff` GTK frontend
and its Linux GPU pipeline are unchanged. This client uses `gliff-proto`,
`gliff-transport` and `gliff-sw` directly and speaks protocol v1. Workspace
default members remain the existing Linux binaries (`gliff`, `gliff-server`,
`gliff-probe`), so a plain Linux packaging build does not add a second frontend.
Workspace-wide checks/tests include this package; Windows builds select
`-p gliff-desktop` explicitly.

## Build and launch

On Windows, install Rust's MSVC toolchain and Visual Studio C++ Build Tools
(including the Windows SDK); OpenH264 builds C++ from source. Windows OpenSSH
Client must be available as `ssh.exe` on PATH. Native Windows CI/build and
real desktop validation remain required; Linux tests are not Windows runtime
evidence.

The distribution build uses the parent integration's packaging script, which
selects the static MSVC CRT:

```powershell
.\scripts\windows\build.ps1
```

For developer builds only (without distribution/static-CRT guarantees):

```powershell
cargo build --release -p gliff-desktop
.\target\release\gliff-desktop.exe user@host
.\target\release\gliff-desktop.exe my-ssh-alias --headless
.\target\release\gliff-desktop.exe my-ssh-alias --output DP-1 --no-clipboard
.\target\release\gliff-desktop.exe my-ssh-alias --server-bin /opt/gliff/gliff-server --video cpu
```

With no host argument, use the address field or reconnect a restored tab.
The remote host needs an installed `gliff-server` and a running Hyprland
session. The default mirrors the selected/focused output; `--output` chooses
an output. `--headless` requests a dedicated output instead. These options
apply to all tabs in this process. Resize requests are sent only after the
server acknowledges an actual headless session, paced at most four per second.

`--connect 127.0.0.1:PORT` (or `[::1]:PORT`) connects directly to an
unauthenticated development listener. Only literal loopback addresses and a
nonzero port are accepted. This mode does not spawn or configure a server;
start `gliff-server --listen` with the desired output options separately.

## Rendering and video

Video decode is CPU H.264 using Single420 or Dual420, with Dual420 preferred
for full chroma when offered by the server. `--video cpu` is the only accepted
mode; VA-API, Vulkan video decode and other hardware decode modes are not
implemented in this frontend. UI composition uses wgpu, separately from CPU
video decode.

Windows enables D3D12 and Vulkan backends, prefers hardware adapters, and keeps
CPU adapters eligible. This avoids requiring an OpenGL 3.3 virtual GPU driver.
Windows 11 may expose Microsoft's WARP D3D12 software adapter. To select an
available CPU adapter explicitly in PowerShell:

```powershell
$env:GLIFF_SOFTWARE_RENDERER = '1'
.\gliff.exe
```

Software adapter availability and presentation in a virtio/other VM depend on
its graphics stack and must be verified on that VM. If no compatible adapter
exists, startup returns an error; this is not a promise that every virtual
display works. The executable does not bundle Mesa or a graphics driver.

## Input and tabs

Click the picture to forward keyboard, pointer buttons, dragging and wheel
input. Shift+Escape releases all held remote keys/buttons and returns input
to the local UI. Clicking local controls, leaving the window, changing tabs,
disconnecting and closing also release remote input. Alt, Ctrl and Shift
states on Windows come from own-window physical messages, including distinct
left/right modifiers. The worker independently tracks held input and attempts
releases plus `Bye` before closing the transport. A broken connection can
prevent delivery; the remote server must also clean up its virtual input when
the stream closes.

On Windows, a winit message observer forwards keyboard transitions only from
Gliff's own UI-thread/window `WM_KEYDOWN/UP` and `WM_SYSKEYDOWN/UP` messages,
alongside that window's mouse button, motion and wheel messages. It
never installs a global keyboard hook, records other windows' input, or consumes
OS messages. Set-1
physical positions map to evdev, including right Ctrl/Alt/Shift, navigation,
Caps/Num Lock, numpad Enter/slash and Windows keys when the OS delivers them.
VK-only synthetic messages use `MapVirtualKeyW` as a best-effort fallback;
without physical scan information they cannot distinguish every position.
E1/Pause and unrecognized scan codes are unsupported. Other layouts, IMEs and
compose/dead-key input are unsupported; the remote keymap remains US QWERTY.
OS-reserved shortcuts such as Ctrl+Alt+Del and many Windows-key combinations
remain local. This adapter does not promise global interception or full parity.

One bounded 256-event queue preserves key/pointer ordering, including Ctrl+click
and click-then-type within one frame, even when egui turns
Ctrl+V/C/X or Insert shortcuts into clipboard events. While captured, the UI
ignores egui key/pointer/clipboard forwarding and generic modifier synthesis;
the ordered native events alone feed the existing held-input lifecycle. Remote repeat comes
from the server keymap; repeated local downs are ignored. With capture off,
local GUI text entry and shortcuts retain their usual egui behavior. The
non-Windows frontend retains its existing egui input path.

An own-window pointer-down marks a provisional capture boundary and samples
only already-held left/right modifiers using this thread's `GetKeyState`.
If egui accepts that click on the picture's own layer, queued events after the click
are retained; earlier local typing is excluded. Local-control clicks discard
provisional events; popup layers never activate the remote picture underneath.
Physical client pixels convert once to egui points using the current DPI before
aspect-fit/remote-scale mapping. Capture leases have epochs: focus loss (including loss and
regain within one frame), tab/connection changes, Shift+Escape or queue overflow
invalidate the lease, discard pending transitions and release remote keys and
buttons. Overflow requires another picture click. Dropping a stale lease cannot
disable a newer capture. OS observation and egui decisions run on the same UI
thread, so accepting a boundary and draining its queue cannot race message
insertion. This behavior still needs native Windows runtime verification.

The picture is aspect fitted with letterboxing. Pointer mapping uses the
displayed frame's geometry and the server's stream scale; local DPI converts
native physical coordinates and sizes headless physical resize requests. A clipped software cursor
uses the remote cursor position and shape. Real DPI/cursor behavior remains
a Windows runtime acceptance item.

Up to eight tabs/workers (including retiring workers) are allowed. Only the
selected tab receives input or shares clipboard contents. Inactive sessions
continue video decode into a single replaceable picture slot. Each tab has
Connect/Disconnect/Reconnect status. Established connections retry at most
twice (three attempts total), with one- and two-second delays. Initial setup,
authentication, host-key and protocol failures stop for explicit Reconnect.

Addresses, tab order and selected tab are saved in
`%APPDATA%\Gliff\desktop.json`. On Linux the path is
`$XDG_CONFIG_HOME/Gliff/desktop.json`, falling back to
`~/.config/Gliff/desktop.json`. Restored tabs are **disconnected** until the user
chooses Reconnect. Keys, passwords and clipboard data are never persisted.
Recent addresses are limited to sixteen; malformed/oversized state files are
ignored. Per-tab server/renderer options and window geometry are not persisted.

## Authentication and clipboard

The desktop SSH launcher reads normal OpenSSH config/agent/key settings, uses
batch key/agent authentication, disables password and keyboard-interactive
prompts and enforces `StrictHostKeyChecking=yes`. Verify the host's fingerprint
in a terminal and establish a trusted known_hosts entry before using the GUI.
Unknown/changed keys fail; the app never silently accepts a new host key. Test
agent/key setup with `ssh -o BatchMode=yes HOST` and check the remote binary's
PATH or use `--server-bin`. Connection setup is limited to fifteen seconds,
OpenSSH connection establishment to ten seconds, and stalled writes to three
seconds. SSH diagnostics retain only the last 8 KiB. Errors appear in the tab.
Remote command arguments are POSIX shell quoted; host text cannot supply SSH
options or shell syntax.

Unicode text clipboard is supported in both directions through the shared
transfer engine, with chunk/window acknowledgements and a 1 MiB text cap.
At most one incoming and one outgoing transfer exist per tab. Native clipboard
calls run on a separate thread; read/write/result slots are bounded. Only the
selected, focused session may offer/fetch text. Leaving it withdraws its offer,
clears retained local text, cancels in-flight transfers and rejects stale
requests. Reactivation requires a fresh local selection offer. Pending native
poll results are tagged to avoid crossing tabs. Clipboard polling is every
500 ms. Text offers are fetched eagerly; image/file clipboard is unsupported.

`--no-clipboard` prevents all remote clipboard offers, requests and transfers
and does not start the native clipboard adapter. Eframe itself may read the
local clipboard when translating a local paste shortcut; that text is never
sent as clipboard data with this flag. Physical Ctrl+V still reaches the remote
application and can paste that remote machine's existing selection.

## Resource and lifecycle bounds

The UI never performs SSH or CPU decode. Each connection has one reader task,
a 64-command input queue, a one-packet reader queue and a latest-wins decoded
picture slot. Input queue overflow cancels the session rather than losing a
release event. Video packets are limited to 16 MiB, CBOR bodies to 1 MiB,
stream/view dimensions to 4096 per side, cursor images to 512 per side and
clipboard chunks to the shared 256 KiB chunk size. Payload declarations must
match frame envelopes. A decoder retains at most four pending geometry entries.
OpenH264 also checks decoded dimensions against the announced stream. These
are finite bounds, not a small aggregate memory guarantee: eight large dual
streams can consume substantial codec/image/GPU memory.

Cancellation aborts the connection's reader and clipboard tasks, attempts
input release/Bye under a total 500 ms deadline, kills SSH and waits up to two
seconds for it to exit. Window updates do not join retiring workers; application
shutdown cancels all workers and joins them after the event loop ends. The
clipboard native thread stops after its current OS call; native clipboard
latency is governed by the operating system.

## Verification status

Automated Linux tests cover real protocol sessions with OpenH264 encode/decode
in both chroma modes, latest picture replacement, physical key mapping and
modifier cleanup, physical chord ordering and both modifier release orders,
Ctrl+Shift+V and Ctrl+V after Shift release, extended/right-side positions,
synthetic scan fallback, provisional capture boundaries, repeat filtering,
overflow/epoch/focus cleanup, letterbox/DPI geometry, headless resize gating,
Unicode clipboard flow/caps/withdrawal, persisted disconnected tabs, framing
limits, unknown messages, bounded retries/timeouts, SSH command quoting/security flags,
authentication/host-key diagnostics, and SSH process termination/reaping.

Commands for this scoped implementation:

```sh
cargo fmt -p gliff-desktop -- --check
cargo clippy -p gliff-desktop --all-targets -- -D warnings
cargo test -p gliff-desktop --all-targets
cargo check -p gliff-desktop --target x86_64-pc-windows-msvc
```

The last command was attempted on Linux with the MSVC Rust target installed,
but OpenH264's C++ build could not find MSVC tools/`lib.exe`. It does not prove
a native Windows build. An isolated adapter-only cross-target check against the
actual Windows eframe/winit/Win32 dependencies passes; it excludes the session
and desktop UI and is not runtime evidence. Native Windows build, installation,
real SSH session, 60-second video, DPI/input/cursor, multi-tab and OS clipboard evidence belong
to the parent validation stream and remain open here. Packaging, CI, public
signing, release and winget publication are outside this implementation.
