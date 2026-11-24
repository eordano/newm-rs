# Wayland protocols

newm-rs serves niri's protocols plus a few of its own: explicit sync (to Wayland clients; X11
windows through xwayland-satellite use implicit sync), output power, the system bell, modal
dialogs, and three KDE protocols ([kde.md](kde.md#wayland-protocols-for-kde-apps)).
Clients in a sandbox (`wp_security_context_v1`) do not see the privileged ones.
[SPEC §9](SPEC.md#9-protocols) lists every global with its version and the protocols not served;
[compat/PROTOCOLS.md](../../compat/PROTOCOLS.md) compares them with KWin and ranks
each gap by what users notice.

Notable gaps: tab dragging between windows (`xdg_toplevel_drag_manager_v1`), per-window icons
(`xdg_toplevel_icon_manager_v1`), colour management and HDR, FIFO and commit timing, pointer warp,
tearing control, and the KDE output and Plasma shell protocols.

## The compat VM suite

`compat/` runs real apps in NixOS VMs against newm-rs: each case starts the desktop as a login session would, opens an app, types into it, and checks
what the app saw (the text, its scale, its buffers). [SPEC §11.3](SPEC.md#113-the-compat-vm-suite)
lists the suites and the expected failures; [compat/README.md](../../compat/README.md) has the
details.

```sh
nix build .#checks.x86_64-linux.compat-newm-rs -L                      # smoke; fails on a failing case
nix build .#checks.x86_64-linux.compat-newm-rs-input -L                # likewise each other suite
nix build .#legacyPackages.x86_64-linux.compat-report.compat-newm-rs   # never fails; writes a report
```

The report holds `results.txt` and `results.json`, the globals each compositor lists, the
journal, screenshots and the probes' output. A case known to fail reports `xfail`, and `xpass`
once it passes.
