# Windows client goal ledger

## Goal and authority

Deliver a Windows client that connects to an existing Omarchy/Hyprland
`gliff-server` over SSH, preserving Rust, the protocol, transport, and software
codec. Work belongs to `spencerbull/gliff`, branch `windows-client`, based on
upstream 32bb062 (0.4.2). Initial worktree was clean.

Allowed: implementation, isolated worker checkouts, fork/branch pushes, CI,
test VM setup on omaprox, nonproduction runtime testing, draft PR in the fork.
Forbidden: upstream changes, production deployment, modifying existing VMs or
user work, billing, secret rotation, publishing a winget submission, pretending
unsigned packages are publicly installable. Code-signing credentials and public
release/submission remain human gates.

## Plan written before implementation

See [Windows specification](docs/windows-plan.md) for architecture, initiatives,
known hurdles, explicit acceptance tests, and evidence requirements.

1. [x] Inspect architecture, manifests, Linux CI, and upstream branches.
   Evidence: existing Rust workspace, GTK/Wayland client, reusable proto,
   transport and gliff-sw; no macOS implementation found.
2. [x] Define Windows scope and validation plan before implementation.
3. [x] Implement Windows desktop client, input, session lifecycle and tests.
   Worker a2f113f integrated as 9bb96b1; native Windows acceptance still open.
4. [x] Implement portable ZIP, MSIX, winget preparation and branch CI.
   Worker 1eeca65 integrated as a542881; fixture/static checks pass, native SDK
   packaging/install checks still open.
5. [x] Establish isolated Windows runtime and a test Hyprland server.
   Live Cua GUI/clipboard/video checks, QGA, pinned-key SSH authentication and
   actual Windows version verified. Product acceptance remains separate.
6. [ ] Pass native Windows build/tests and Linux regression checks.
7. [ ] Capture real session and installer screenshot/video evidence.
8. [ ] Independent review, fix verified findings, repeat affected checks.
9. [x] Push branch and open a draft PR in spencerbull's fork.
   https://github.com/spencerbull/gliff/pull/1 (draft; verification ongoing).

## Work streams and ownership

Backend: T3 Code. Parent owns integration, root Cargo manifests/lockfile, plan,
README, regression checks, test infrastructure and evidence. Workers may not
merge, deploy, publish releases, or start further workers. Every writer receives
a T3-managed isolated worktree. Cleanup owner for all workers is parent.

| Stream | Scope | Worker / checkout / branch | State and evidence |
| --- | --- | --- | --- |
| Desktop | New `crates/gliff-desktop/` Rust frontend and protocol session | T3 child `windows-desktop-client`, task suffix `0fd12e91-70c6-4251-93c8-c078bd45e52d`; worktree `t3-22515915-fcdf078a1d1de8d9`, branch `t3/windows-rust-desktop-client` | a2f113f inspected and integrated as 9bb96b1. Worker fmt/clippy/104 portable tests passed. Parent reruns full workspace and native CI; Windows cross-build alone blocked on absent MSVC tools, not treated as proof. |
| Distribution | `packaging/windows/`, `scripts/windows/`, Windows CI | T3 child `windows-distribution-ci`, task suffix `347096b4-ff93-4711-a7e4-a7ca367d0062`; worktree `t3-1342a5d8-4b939796137f936f`, branch `t3/windows-distribution-ci` | Worker commit 1eeca65 inspected and integrated as a542881. Parent reran all 25 PowerShell fixture assertions, parsed 8 scripts, actionlint, 3 Microsoft winget schema validations and PNG dimension/CRC checks; PASS. Native Windows packaging not yet run. |
| Runtime | Windows VM and real remote session evidence | Parent, local isolated Docker/KVM fallback | omaprox T3 link pending; tailnet denies root, sbull absent; local host has 105 GiB available RAM and 1.5 TiB disk |
| Runtime readiness | Interactive Windows Cua Driver and SSH setup; no product/repo edits | T3 child `windows-runtime-readiness`, task suffix `283e391b-3bb0-481e-aa9b-1413216a0f8b`, shared checkout read-only; exclusive external `runtime-worker/` artifacts | Completed: Win11 26300.9457, QGA, live interactive Cua/clipboard/MP4, SSH pinned-key auth. Report `/tmp/gliff-windows-infra/runtime-worker/REPORT.md`. Its gliff-server probe omitted --stdio; actual client supplies it. |
| Review | Independent distribution review | T3 child `windows-packaging-review`, task suffix `fbe6c910-fcb9-4b9f-b020-d3c8daa45a5f`, shared checkout read-only | Completed: one P2, portable WinGet stanza must omit Scope. Parent verified upstream WinGet semantic validator, added failing regression, removed Scope only for portable; all 26 fixtures and 3 schemas now pass. Native winget CLI check pending. |
| Desktop review | Independent client review | T3 child `windows-desktop-independent-review`, task suffix `06dcfbcb-0632-411a-8abe-9f8648491047`, shared checkout read-only | Completed: four P2 findings (late remote clipboard, reconnect geometry, reconnect offer, empty clipboard paste). Parent verified source/probes and added selection generations, connection epochs, modifier history, regressions. Final checks in progress. |

## Required gates and open questions

- Run `scripts/check.sh` before every push (repository CLAUDE.md). Local
  GTK 4.24.1 and libadwaita 1.10.0 are installed (`libadwaita-1` is the correct
  pkg-config name); baseline check PASSED, log `/tmp/gliff-baseline-check.log`.
- Windows native rustfmt/clippy/tests/build, ZIP/MSIX validation and install
  smoke test; Linux workspace checks; hardware e2e if video core changes.
- Capture screenshots and video from real Windows, with commit/build identity
  and an observable remote input result. Synthetic fixtures complement this;
  they do not substitute for Windows-to-Omarchy evidence.
- omaprox is online in Tailscale; T3 connection requested, access pending.
- Fork creation initially hit a gh flag incompatibility (`--remote=false`
  cannot accompany a repository argument); corrected to documented syntax.
- Release publisher identity/signing certificate not supplied; unsigned MSIX
  artifacts and documented local test signing are acceptable for CI, while
  trusted distribution and winget submission are explicit remaining gates.
- Mac support is future work; no Mac build is claimed by this change.
- Git author identity was absent; set repository-local verified GitHub identity
  `Spencer Bull <7035687+spencerbull@users.noreply.github.com>` for commits.
- Parent cross-checked shared protocol/transport (including test targets) for
  `x86_64-pc-windows-msvc`. Found and fixed a Unix-only `unused_mut` warning in
  spool directory construction. Windows-target Clippy with `-D warnings`,
  rustfmt, and all 33 transport tests now pass. Logs in
  `/tmp/gliff-windows-infra/{shared-windows-clippy,transport-tests}.log`.
- Parent early client review found missing clipboard withdrawal from inactive
  tabs and missing restored tabs on startup; sent verified findings to desktop
  worker before integration. egui does not expose Windows Super through
  `Modifiers.mac_cmd`; worker must implement or document that limitation.
- Follow-up source inspection confirms explicit active/inactive clipboard
  state, withdrawal and cancellation, saved tabs/selection restoration, and
  meaningful real-codec/session tests are now implemented in the worker
  checkout. Parent will rerun tests and inspect its final commit before accepting.
- Windows bootstrap ran through a native TigerVNC window using host Cua Driver
  in standard mode. No raw QMP input injection or host driver settings changed.
  Guest CIM evidence confirms Windows 11 Enterprise Evaluation 64-bit,
  10.0.26300, user GLIFF-WINTEST\\glifftest in interactive session 1. Live
  automation readiness is still being verified by the runtime worker.
- Requested explicit Linux workspace default members (GTK client, server,
  probe) so `bin/build` does not build an unused second frontend for the Arch
  package. Windows builds select gliff-desktop; checks still cover workspace.
- Parent reproduced a reconnect bug with a new real supervisor/TCP test: an
  established connection followed by a failed reconnect setup stopped before
  the third attempt. Preserve established-session retry eligibility across
  transient setup failures. Test failed before fix; all 13 session integration
  tests now pass, log `/tmp/gliff-windows-infra/session-tests-after-fix.log`.
- Integrated `scripts/check.sh` passed all 301 tests before first native run.
  OAuth HTTPS push could not update workflows without workflow scope; verified
  existing GitHub SSH authentication, switched only fork remote to SSH and
  pushed successfully. Fork Actions initially had no registered workflows;
  applying its enabled/all-actions setting registered CI. No token scope or
  global credentials changed.
- First native run https://github.com/spencerbull/gliff/actions/runs/42636044509:
  Linux jobs PASS; Windows MSVC compilation, Clippy and client tests PASS.
  Shared file-transfer test FAILED because Windows serialized native backslash
  paths. Fixed actual wire path serialization to join native components with
  '/', preserving the existing cross-platform test instead of skipping it.
- Added separate `windows-msix-test` CI artifact with short-lived, self-signed
  install/upgrade fixtures and public CER. No private key export or host trust
  changes; public release jobs cannot consume it. This supports clean-VM MSIX
  testing without installing an SDK. Native SDK/signing execution still pending.
- cee0861 PR run 42638509177 produced all Windows artifacts. Parent downloaded
  ZIP/MSIX/test-signed fixtures and verified every checksum. Native MSVC,
  Clippy/tests, import checks and SDK structural validation passed. Clean guest
  --version/hash probe passed with no Rust/VS/SDK installed.
- First actual Windows GUI launch FAILED surface creation. Captured stderr
  proved no requested backend initialized; cargo feature inspection showed
  eframe disables Direct3D defaults. Added explicit Windows-only wgpu dx12
  feature plus native compiled-backend guard; actual GUI retest pending.
- Follow-up reviewer `windows-fixes-and-msix-review`, task suffix
  `02c7c861-17f1-48c0-aa59-ce49421b8344`, found two remaining paste release-order
  bugs and no new clipboard/epoch/MSIX fixture defects. Parent reproduced both
  with new failing tests, retained full modifier chords through releases and
  batched events; all six input tests now pass. Full checks/native rerun pending.
- Further input analysis identified egui's missing original key-down/modifier
  timing as the source of ambiguous shortcut reconstruction. A bounded Windows
  adapter will capture only messages delivered to Gliff's own winit event loop,
  forward physical keys directly and keep reserved Windows shortcuts local.
  No global keyboard hook. Separate worker/worktree owns it while parent tests
  the already-built renderer and installer; final Windows input acceptance will
  use that adapter rather than heuristics.
- Native WinGet CLI validation passed for fixed fixture manifests, while the
  identical Scope:user negative control returned nonzero with the expected
  warning. Raw evidence is committed in docs/evidence/windows/.
- Fork branch published through d35f895 after full Linux checks passed again.
  Initial HTTPS push lacked a credential helper; command-scoped authenticated
  `gh auth git-credential` helper worked without changing global Git settings.

## Runtime resources created by this loop

- Container `gliff-windows-test` on the local host (omaprox unavailable), pinned
  image `dockurr/windows@sha256:16fd7c39a3e48bb946d29c59dc6015942e5b900a1fa5d7e5552053bfc48ee2ea`.
  Windows 11 Enterprise evaluation from Microsoft, 8 cores / 16 GiB / 100 GB;
  VM data under `~/.local/share/gliff-windows-test/`. Console 127.0.0.1:18006,
  RDP 127.0.0.1:13389, guest SSH forward 127.0.0.1:12222. No production
  volumes/devices attached, no host network or NET_ADMIN; user-mode networking.
- Nested Hyprland PID 1701026, instance suffix `1794232916_1395406566`,
  socket `wayland-2`. Only this new fixture is used for remote capture/input.
- Unprivileged test sshd PID recorded in `/tmp/gliff-windows-infra/server/sshd.pid`,
  listening on Docker bridge 172.17.0.1:22222. Ephemeral generated key only,
  forced command constrained to Gliff against the nested test session;
  no password login, forwarding or PTY. Existing authorized_keys untouched.
  Verified SSH authentication and actual gliff-server launch over this path.
- Passt maps the guest gateway to container loopback, and host firewall blocks
  direct container-to-host connections. Parent added an isolated reverse relay:
  guest/container loopback 22222 pairs with host-initiated channels on internal
  container port 22223, then forwards to the local test sshd. No host firewall
  change and no published port. Processes are owned by exec sessions 85496 and
  97783; scripts `/tmp/gliff-windows-infra/reverse-relay-{container,host}.py`.
  Verified real Windows SSH banner and pinned-key authentication through it.
- Windows native GUI verification will use Cua Driver (computer-use skill).
  Test-only Windows binary 0.45.2 downloaded from trycua/cua release and SHA256
  checked against release checksums; no host driver installation changes.
- Cleanup owner: parent. Stop container, test sshd, nested compositor and its
  children after evidence collection; retain test disk only for continued user
  testing, document final resource state. Never stop unrelated sessions.
- Pre-MSIX crash-consistent disk checkpoint completed through QEMU's live backup
  job (`data3`, full 100 GB logical range). Backup job concluded without error,
  dismissed; `qemu-img check` passed for
  `~/.local/share/gliff-windows-test/storage/checkpoints/pre-msix.qcow2`.
  This is a disk checkpoint, not a RAM or VSS/application-consistent snapshot.

## Runtime checkpoint after renderer fix

- a097f81 branch and PR CI passed (PR run 42641834504). Downloaded all three
  artifacts and verified portable ZIP hash in the clean VM. GUI launch now
  passes with Direct3D; real Windows SSH/H.264 video displays the isolated
  Omarchy fixture. Screenshot and raw input/build evidence are committed in
  docs/evidence/windows/a097f81-*.
- Remote terminal logged Windows mouse motion/press/release and typed `a`.
  Full W04 input acceptance remains pending the native keyboard adapter.
- First video decodes successfully (110.733 seconds) but the setup terminal
  obscures most frames; it does not satisfy the visible 60-second W03 gate.
  Local media: shared/acceptance/evidence/recording.mp4.
- Unicode clipboard test crashed Cua's test transport on invalid UTF-8.
  Fixed broker legacy stdin/JSON encodings, quarantined pending failed requests
  without replay, restarted only owned broker, and verified exact Unicode
  Windows clipboard write/read. Omarchy propagation remains open. Recorder
  confirmed disabled; Gliff PID 6720 remains live for parent-controlled tests.
- scripts/check.sh for a097f81 passed 307 tests (1 existing ignored). Native
  MSVC/Clippy/tests/SDK checks passed in CI; adapter changes will need reruns.
