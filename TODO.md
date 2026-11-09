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
| Desktop | New `crates/gliff-desktop/` Rust frontend and protocol session | Not started | Spec ready |
| Distribution | `packaging/windows/`, `scripts/windows/`, Windows CI | Not started | Spec ready |
| Runtime | Windows VM and real remote session evidence | Parent initially | omaprox machine-link requested |
| Review | Independent source and packaging review | Not started | After integration |

## Required gates and open questions

- Run `scripts/check.sh` before every push (repository CLAUDE.md); local
  libadwaita pkg-config dependency currently absent, repair environment first.
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
