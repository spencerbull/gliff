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
3. [ ] Implement Windows desktop client, input, session lifecycle and tests.
4. [ ] Implement portable ZIP, MSIX, winget preparation and branch CI.
5. [ ] Establish isolated Windows runtime and a test Hyprland server.
6. [ ] Pass native Windows build/tests and Linux regression checks.
7. [ ] Capture real session and installer screenshot/video evidence.
8. [ ] Independent review, fix verified findings, repeat affected checks.
9. [ ] Push branch and open a draft PR in spencerbull's fork.

## Work streams and ownership

Backend: T3 Code. Parent owns integration, root Cargo manifests/lockfile, plan,
README, regression checks, test infrastructure and evidence. Workers may not
merge, deploy, publish releases, or start further workers. Every writer receives
a T3-managed isolated worktree. Cleanup owner for all workers is parent.

| Stream | Scope | Worker / checkout / branch | State and evidence |
| --- | --- | --- | --- |
| Desktop | New `crates/gliff-desktop/` Rust frontend and protocol session | T3 child `windows-desktop-client`, task suffix `0fd12e91-70c6-4251-93c8-c078bd45e52d`; worktree `t3-22515915-fcdf078a1d1de8d9`, branch `t3/windows-rust-desktop-client` | Implementation started; parent integrates commits |
| Distribution | `packaging/windows/`, `scripts/windows/`, Windows CI | T3 child `windows-distribution-ci`, task suffix `347096b4-ff93-4711-a7e4-a7ca367d0062`; worktree `t3-1342a5d8-4b939796137f936f`, branch `t3/windows-distribution-ci` | Implementation started; parent integrates commits |
| Runtime | Windows VM and real remote session evidence | Parent, local isolated Docker/KVM fallback | omaprox T3 link pending; tailnet denies root, sbull absent; local host has 105 GiB available RAM and 1.5 TiB disk |
| Review | Independent source and packaging review | Not started | After integration |

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
- Windows native GUI verification will use Cua Driver (computer-use skill).
  Test-only Windows binary 0.45.2 downloaded from trycua/cua release and SHA256
  checked against release checksums; no host driver installation changes.
- Cleanup owner: parent. Stop container, test sshd, nested compositor and its
  children after evidence collection; retain test disk only for continued user
  testing, document final resource state. Never stop unrelated sessions.
