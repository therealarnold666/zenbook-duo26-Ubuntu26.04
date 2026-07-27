# ASUS Zenbook Duo UX8407AA on Ubuntu 26.04

Linux support tools and kernel workarounds for the 2026 ASUS Zenbook Duo
UX8407AA. The project coordinates the detachable keyboard, lower display,
backlights, orientation, hotkeys, and Bluetooth behavior through a Rust
daemon, a per-user session agent, and a Tauri control panel.

> [!IMPORTANT]
> The UX8407AA is the primary tested machine. Userspace support is usable,
> but the lower-panel kernel workaround is still an out-of-tree candidate,
> not an accepted upstream Linux fix.

## Current Status

The current known-good test system is Ubuntu 26.04 with GNOME on Wayland:

- Machine: ASUS Zenbook Duo UX8407AA
- Kernel: `7.2.0-rc4-zenbook-ab72tcsshold1`
- Kernel base: Linux 7.2-rc4
- Validation parameters retained during testing:
  `xe.enable_psr=0 xe.enable_panel_replay=0`
- Display scale: `1.67x`
- Runtime services: system daemon, lifecycle helper, and user session agent

Repeated cold boots, long GDM waits, desktop handoff, VS Code startup,
keyboard attach/detach, and lower-panel disable/re-enable no longer reproduce
the previous Pipe B/PHY B freeze on the test machine.

The custom kernel is not installed by `install.sh`. The exact tested source
delta is provided in
[`patches/kernel/0001-ux8407aa-port-b-tcss-power-and-diagnostics.patch`](patches/kernel/0001-ux8407aa-port-b-tcss-power-and-diagnostics.patch).

## Features

| Feature | Status | Notes |
| --- | :---: | --- |
| Lower display off when keyboard is attached | Yes | GNOME, KDE, and Niri backends |
| Lower display on when keyboard is detached | Yes | Reopens only when the runtime owns the previous disable |
| Bluetooth recovery on keyboard detach | Yes | Does not change Wi-Fi state |
| USB and Bluetooth keyboard detection | Yes | UX8407AA USB ID `0b05:1cd7` |
| Keyboard backlight boot/attach restore | Yes | Includes attach/detach state synchronization |
| Display brightness synchronization | Yes | Uses session display interfaces and sysfs helpers |
| Orientation controls | Yes | Corrects the main panel's physical 180-degree mounting baseline |
| Dual-screen left/right arrangement | Yes | Uses the UX8407AA physical panel order |
| Automatic sensor rotation | Partial | Session agent handles `monitor-sensor`; no UI on/off switch yet |
| `1.67x` display scaling | Yes | Default and selectable in the control panel |
| Media and ASUS function keys | Yes | USB and Bluetooth support varies by key |
| Control panel and tray icon | Yes | Tauri/React application |
| Suspend/resume and session recovery | Yes | Lifecycle and startup replay are boot-aware |

The runtime intentionally does not toggle Wi-Fi. On keyboard detach it only
ensures that Bluetooth is powered, so the detached keyboard remains usable.

## Install

Run the installer from an active GNOME, KDE Plasma, or Niri Wayland session:

```bash
curl -fsSL https://raw.githubusercontent.com/therealarnold666/zenbook-duo26-Ubuntu26.04/main/install.sh | bash
```

Or clone the repository:

```bash
git clone https://github.com/therealarnold666/zenbook-duo26-Ubuntu26.04.git
cd zenbook-duo26-Ubuntu26.04
./install.sh
```

The installer:

- Detects GNOME, KDE Plasma, or Niri.
- Installs compositor and sensor dependencies.
- Installs Rust through rustup when Cargo is unavailable.
- Builds and installs the four Rust runtime binaries.
- Enables the system daemon, lifecycle service, and session agent.
- Builds and installs the Tauri control panel unless `--skip-ui` is used.
- Adds required udev, input-group, backlight, and autostart integration.

Log out and back in after the first install so group and session environment
changes take effect.

Useful alternatives:

```bash
./install.sh --skip-ui
./setup-gnome.sh
./setup-kde.sh
./setup-niri.sh
./install-ui.sh
```

## Verify

```bash
uname -r
systemctl is-active zenbook-duo-rust-daemon.service
systemctl is-active zenbook-duo-rust-lifecycle.service
systemctl --user is-active zenbook-duo-session-agent.service
```

All three services should report `active`. Attach and detach the keyboard and
confirm that the lower display follows it and Bluetooth remains available.

## Two Separate Kernel Issues

### Spurious airplane mode

UX8407AA firmware emits ASUS WMI events `0x5d`, `0x5e`, and `0x5f` during
keyboard state changes. Kernels missing the UX8407AA DMI quirk may expose
them as RF-kill keys and toggle airplane mode.

The optional DKMS backport is included in
[`ux8407aa-asus-nb-wmi-quirk`](ux8407aa-asus-nb-wmi-quirk/README.md).
Install it only when the running kernel does not already contain the upstream
UX8407AA match.

### Intermittent eDP-2 black screen

The lower panel could remain logically enabled while becoming black and
unresponsive. Diagnostic snapshots showed Panther Lake Port B C20 PHY losing
PLL/reference-clock acknowledgement while software requests remained set.

The tested workaround keeps the Port B TCSS power request asserted before
C20 PLL programming. It is restricted by DMI, platform, output type, port,
and PHY type to the UX8407AA lower-panel path. The known-good patch also
retains Pipe B DSB/FlipQ mitigation and timeout diagnostics used during
validation.

Apply it to a clean Linux 7.2-rc4 source tree:

```bash
patch -p1 < /path/to/zenbook-duo26-Ubuntu26.04/patches/kernel/0001-ux8407aa-port-b-tcss-power-and-diagnostics.patch
```

Build with a unique local version and keep a distribution kernel installed
as a GRUB fallback. See
[`UX8407AA_KERNEL_AND_RUNTIME_FIXES_2026-07-27.md`](UX8407AA_KERNEL_AND_RUNTIME_FIXES_2026-07-27.md)
for evidence, failed experiments, and validation scope.

## Runtime Design

- `zenbook-duo-rust-daemon.service`: hardware state, policy, persistence, and
  IPC coordination.
- `zenbook-duo-rust-lifecycle.service`: boot, shutdown, suspend, and resume
  hooks.
- `zenbook-duo-session-agent.service`: compositor commands, hotkeys,
  accelerometer events, notifications, and session-owned display work.
- `zenbook-duo-control`: Tauri/React control panel and tray application.

Display ownership includes the current kernel boot ID. It survives daemon
restarts during one boot but is discarded after reboot, preventing stale
state from reopening a display unexpectedly. USB polling reads sysfs directly
instead of running `lsusb` every second, avoiding repetitive AppArmor denials.

## Troubleshooting

Watch the runtime:

```bash
journalctl -u zenbook-duo-rust-daemon.service -f
journalctl --user -u zenbook-duo-session-agent.service -f
```

Restart the session component after an update:

```bash
systemctl --user restart zenbook-duo-session-agent.service
```

If the lower panel is logically enabled but black, or the kernel reports
`flip_done`, PHY B, DPLL, or link-training timeouts, treat it as a kernel
display failure rather than repeatedly replaying userspace layouts.

If `KBLIGHT - Device lost, re-scanning` repeats after installation, log out
and back in so the session receives the new `input` group membership.

If an old USB hwdb remap is still installed, remove it because it overrides
the keyboard's Fn layer:

```bash
sudo rm -f /etc/udev/hwdb.d/90-zenbook-duo-keyboard.hwdb
sudo systemd-hwdb update
sudo udevadm trigger
```

## Development

After build caches have been cleaned, restore dependencies and run checks:

```bash
cd ui-tauri-react
npm ci
npm run vite:build
cd src-tauri
cargo test --lib
```

Build the desktop package with:

```bash
cd ui-tauri-react
npm run build -- --bundles deb
```

Generated `node_modules`, Rust `target`, packages, logs, kernel source trees,
and diagnostic captures are excluded by the root `.gitignore`.

## Documentation

- [Kernel and runtime fix summary](UX8407AA_KERNEL_AND_RUNTIME_FIXES_2026-07-27.md)
- [eDP-2 keyboard A/B report](XE_EDP2_KEYBOARD_AB_REPORT.md)
- [Greeter display investigation](KEYBOARD_BOOT_GREETER_DISPLAY_INVESTIGATION.md)
- [Post-login recovery findings](POST_LOGIN_DISPLAY_RECOVERY_FINDINGS.md)
- [Main-panel rotation investigation](MAIN_PANEL_ROTATION_INVESTIGATION.md)
- [Keyboard backlight compatibility notes](BACKLIGHT_COMPATIBILITY_NOTES_2026-07-24.md)

This project builds on the earlier community work in
[Fmstrat/zenbook-duo-linux](https://github.com/Fmstrat/zenbook-duo-linux) and
the subsequent [`zakstam/zenbook-duo-linux`](https://github.com/zakstam/zenbook-duo-linux)
fork.

## Uninstall

```bash
./uninstall.sh
```

Remove the control-panel package separately when required:

```bash
sudo apt remove zenbook-duo-control
```
