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
- Managed brightness parameter: `xe.enable_dpcd_backlight=3`
- Additional validation parameters retained on the test machine:
  `xe.enable_psr=0 xe.enable_panel_replay=0`
- Display scale: `1.67x`
- Control panel version: `0.3.0`
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
| Display brightness synchronization | Yes | Keeps the last selected percentage across keyboard attach/detach and lower-panel transitions |
| Performance profiles and battery saver | Yes | Quiet, Balanced, and Performance profiles; optionally enters Quiet on battery and restores the previous mode on AC |
| Battery charge protection | Yes | Live Wh/power telemetry and persistent 80%, 90%, or 100% charge limits |
| Internal audio | Kernel + UCM patches | Ghost RT722 quirk and CS35L56 + CS42L43 routing; see below |
| Orientation controls | Yes | Corrects the main panel's physical 180-degree mounting baseline |
| Dual-screen left/right arrangement | Yes | Uses the UX8407AA physical panel order |
| Automatic sensor rotation | Yes | Optional Controls switch; rotates both enabled panels from the Intel ISH accelerometer |
| `1.67x` display scaling | Yes | Default and selectable in the control panel |
| Media and ASUS function keys | Yes | USB and Bluetooth support varies by key |
| F12 Control shortcut | Yes | Brings the Zenbook Duo Control window to the foreground, or starts it if needed |
| Control panel and tray icon | Yes | Tauri/React application |
| Suspend/resume and session recovery | Yes | Lifecycle and startup replay are boot-aware |

The runtime intentionally does not toggle Wi-Fi. On keyboard detach it only
ensures that Bluetooth is powered, so the detached keyboard remains usable.

### Automatic dual-screen rotation

The **Controls → Screen Orientation** card includes an **Auto rotate** switch.
It is off by default. When enabled, the session agent reads the Intel ISH
accelerometer directly and rotates both panels only while two GNOME logical
monitors are active. This avoids accidental layout changes in single-screen
or keyboard-attached mode.

The UX8407AA needs working Intel ISH sensor firmware. Confirm the sensor is
available with:

```bash
monitor-sensor --accel
```

For systems whose firmware does not publish a mount matrix, install the
included local udev rule and restart the proxy once:

```bash
sudo install -D -m 0644 system/udev/61-asus-ux8407aa-sensors.rules \
  /etc/udev/rules.d/61-asus-ux8407aa-sensors.rules
sudo udevadm control --reload-rules
sudo udevadm trigger --subsystem-match=iio --sysname-match='iio:device0'
sudo systemctl restart iio-sensor-proxy.service
systemctl --user restart zenbook-duo-session-agent.service
```

### Touchscreen alignment

The main panel is physically mounted 180 degrees from its usable display
orientation. The GNOME setup script installs a libinput calibration rule for
the primary RAYD touchscreen, and persists each internal touchscreen's GNOME
output assignment. After a manual rule installation, reboot once (or rebind
the touchscreen device) before testing it.

Rotate the device left or right while keeping the displays facing you; a
face-up or face-down position is intentionally ignored because it has no
unambiguous screen orientation.

### Battery charge protection

The Status page includes a Battery card with the current charge in Wh,
discharge power in W, charging state, and an 80%, 90%, or 100% charge-limit
selector. The system daemon writes the selected limit through the kernel's
standard ASUS battery interface:

```text
/sys/class/power_supply/BAT0/charge_control_end_threshold
```

The selection is saved in `~/.config/zenbook-duo/settings.json` and restored
by the root daemon at startup. Linux 7.2 may initially report this ASUS
threshold as unknown because the firmware cannot read it back; applying the
saved value makes it readable for the current boot. The UI displays
`Not applied` rather than claiming protection when the kernel does not confirm
the requested value. A 100% limit restores normal full charging.

### Performance profiles and battery saver

The **Controls → Performance Mode** card offers Quiet, Balanced, and
Performance profiles. Its **Battery saver** switch is off by default. When
enabled, the daemon records the selected profile and switches to Quiet only
while `BAT0` reports `Discharging`. On AC power (including a connected adapter
that reports `Not charging`), it restores the recorded profile. A manual
profile selection cancels a pending automatic restore.

### Brightness and F12 shortcut

Brightness is stored as a percentage and applied to every active internal
panel. The session agent ignores the brief firmware maximum-brightness report
that can occur while the keyboard is attached or detached, then reapplies the
saved level once the display topology is stable. Brightness hotkeys update the
same saved value.

F12 opens Zenbook Duo Control. If the control panel is already running in the
tray, F12 restores it and brings it to the foreground. The mapping works for
both the docked USB keyboard and the detached Bluetooth keyboard.

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
- Configures `xe.enable_dpcd_backlight=3` on UX8407AA through a managed GRUB drop-in.
- Installs Rust through rustup when Cargo is unavailable.
- Builds and installs the four Rust runtime binaries.
- Enables the system daemon, lifecycle service, and session agent.
- Builds and installs the Tauri control panel unless `--skip-ui` is used.
- Adds required udev, input-group, backlight, and autostart integration.

Reboot after the first install so the xe backlight parameter takes effect,
then log out and back in if group or session environment changes still need
to be refreshed.

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

Check the managed xe backlight parameter with:

```bash
./tools/configure-xe-backlight.sh status
```

`grub-configured=yes` means the next boot will include it;
`running-kernel-active=yes` means the current boot already includes it.

## Kernel and Driver Requirements

### DPCD screen brightness

The UX8407AA panels expose brightness control through the eDP DisplayPort AUX
channel. This project installs `xe.enable_dpcd_backlight=3`, which tells xe to
force the Intel DPCD backlight interface. Without it, the expected DRM
backlight node may be absent or brightness changes may not reach the panel.

The installer writes only
`/etc/default/grub.d/90-zenbook-duo-xe-backlight.cfg`; it does not rewrite
`/etc/default/grub` or own the PSR/Panel Replay options. Manage it manually
when needed:

```bash
./tools/configure-xe-backlight.sh install
./tools/configure-xe-backlight.sh status
./tools/configure-xe-backlight.sh remove
```

A reboot is required after `install` or `remove`.

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

### Missing internal audio

UX8407AA firmware reports a non-existent RT722 on SoundWire link 3 alongside
the real CS42L43. Both advertise a `SimpleJack` function, so `sof_sdw` tries
to register `SDW3-Playback-SimpleJack` twice. ALSA card registration then
fails with `-EEXIST`, leaving PipeWire with only `Dummy Output`.

The kernel already removes the same ghost RT722 on several Panther Lake
machines. This project extends that DMI quirk to the UX8407AA in
[`patches/kernel/0002-ux8407aa-ignore-ghost-rt722.patch`](patches/kernel/0002-ux8407aa-ignore-ghost-rt722.patch).
It preserves the real CS42L43, CS35L56 amplifiers, microphone, and HDMI audio.

Apply `0001` and `0002` when building a complete custom kernel. To test only
the audio fix against the currently running, matching 7.2-rc4 kernel:

```bash
./tools/build-audio-quirk.sh /path/to/linux-7.2-rc4
sudo ./tools/build-audio-quirk.sh install /path/to/linux-7.2-rc4
sudo reboot
```

When Secure Boot is enabled, the installer signs the override with Ubuntu's
already enrolled DKMS MOK under `/var/lib/shim-signed/mok/`. It fails safely
instead of installing an unsigned module when that key is unavailable.

After reboot, `aplay -l` should show a SOF SoundWire card. Ubuntu 26.04's
`alsa-ucm-conf` still predates upstream support for the combined
`spk:cs35l56+cs42l43-spk` component string. Without the UCM backport,
WirePlumber falls back to `Jack Out` even though the ALSA card exists.

Install and activate the project-managed UCM backport:

```bash
./tools/configure-audio-ucm.sh validate
sudo ./tools/configure-audio-ucm.sh install
./tools/configure-audio-ucm.sh activate
```

`wpctl status` should now show `sof-soundwire Speaker` and
`sof-soundwire Microphones`. The installer backs up the two distribution
files it modifies under `/var/lib/zenbook-duo/audio-ucm-backup`.

Remove both audio overrides if needed:

```bash
sudo ./tools/configure-audio-ucm.sh remove
sudo ./tools/build-audio-quirk.sh remove
sudo reboot
```

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
