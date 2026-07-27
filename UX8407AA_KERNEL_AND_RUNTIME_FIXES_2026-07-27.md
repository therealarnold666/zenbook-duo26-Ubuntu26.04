# ASUS Zenbook Duo UX8407AA: Kernel and Runtime Fixes

Date: 2026-07-27

This document records the main changes made while investigating the ASUS
Zenbook Duo UX8407AA on Ubuntu 26.04. The most important result is an
out-of-tree Linux display-driver fix for the lower internal panel (`eDP-2`).
It also summarizes the project runtime changes that were made during the
same investigation.

## Current Known-Good Environment

- Machine: ASUS Zenbook Duo UX8407AA
- Kernel base: Linux 7.2-rc4
- Installed kernel: `7.2.0-rc4-zenbook-ab72tcsshold1`
- Kernel parameters retained during validation:
  `xe.enable_psr=0 xe.enable_panel_replay=0`
- Project services:
  `zenbook-duo-rust-daemon.service`,
  `zenbook-duo-rust-lifecycle.service`, and
  `zenbook-duo-session-agent.service`
- The final test state had all three services active.

The kernel patch used by this build is stored at:

`patches/kernel/0001-ux8407aa-port-b-tcss-power-and-diagnostics.patch`

## Original eDP-2 Failure

The lower panel could become black while desktop settings still reported it
as enabled. The pointer could sometimes move into the lower logical display,
but the panel did not scan out new content. Related symptoms included:

- A delayed black screen in GDM or shortly after entering the desktop.
- A failure triggered more often by display mode changes or opening an
  application such as VS Code.
- Long display-configuration stalls and frozen password dialogs.
- Slow or inhibited shutdown/reboot after the display failure.
- `flip_done` timeouts and Panther Lake Port B/PHY B errors in the kernel log.

Stopping all Zenbook Duo project services did not eliminate the GDM failure.
This isolated the original black-screen trigger from the userspace
attach/detach policy and showed that the kernel display path had an
independent fault.

## Kernel Investigation

The experiments progressed from Linux 7.0.28 through a custom Linux 7.1.5
build and finally Linux 7.2-rc4. Updating the base kernel alone did not make
the intermittent failure disappear.

A diagnostic 10-second atomic flip wait captured Pipe A and Pipe B register
state at the point of failure. The important comparison was:

- Healthy PHY A clock control: `0xf0008400`, with PLL and reference-clock
  request/acknowledge state present.
- Failed PHY B clock control: `0xa0008400`, with software requests still
  asserted but PLL and reference-clock acknowledgements absent.
- PHY B `BUF_CTL2`: `0x3c220020`, whose PHY current-status differed from the
  healthy Port A state (`0x0c222220`).

The evidence showed that software display-clock requests remained asserted,
but the Panther Lake Port B C20 Type-C PHY had autonomously lost its usable
power/clock response. A normal full retrain after the failure could not
reliably recover it.

On this model, the lower internal panel is connected as `eDP-2` through
Panther Lake Port B's C20 Type-C PHY path. The working change keeps the TCSS
PHY power request asserted before programming the C20 PLL and waits up to
5 ms for the TCSS power-state acknowledgement.

The workaround is deliberately restricted to:

- ASUS system vendor.
- Product name `Zenbook Duo UX8407AA`.
- Panther Lake.
- An eDP encoder on Port B.
- The C20 PHY path, not C10.

## Kernel Source Changes

The known-good build changes four i915 display files:

| File | Purpose |
| --- | --- |
| `drivers/gpu/drm/i915/display/intel_cx0_phy.c` | Detect the UX8407AA Port B C20 eDP path, assert TCSS power before PLL programming, and warn if it is not acknowledged. |
| `drivers/gpu/drm/i915/display/intel_tc.c` | Expose a reusable helper for changing the TCSS power request. |
| `drivers/gpu/drm/i915/display/intel_tc.h` | Declare the TCSS power-request helper. |
| `drivers/gpu/drm/i915/display/intel_display.c` | Retain Pipe B DSB/FlipQ mitigation and timeout-time register diagnostics used by the validated build. |

The TCSS power hold was the change after which the repeated failure stopped.
The Pipe B DSB/FlipQ override and extended flip diagnostics were introduced
earlier for isolation; they did not solve the issue by themselves. They are
included in the patch because they are part of the exact tested kernel, but
they should be separated or removed before proposing a minimal upstream
change.

The following experiments did not independently resolve the failure:

- Disabling PSR and Panel Replay.
- DSB-only enable/disable variants.
- Display power-well variants.
- Retrying link training after the PHY had already failed.
- P2-ready and power-down state normalization attempts.

## Validation Result

After installing `7.2.0-rc4-zenbook-ab72tcsshold1`, repeated tests covered:

- Multiple consecutive cold boots.
- Long waits at GDM.
- GDM-to-desktop handoff.
- Starting VS Code and interacting with password prompts.
- Keyboard attach and detach.
- Lower-panel disable and re-enable.

The final rounds did not reproduce Pipe B/PHY B/`flip_done` failures or the
previous desktop freeze. This is strong machine-local evidence, not proof
that the workaround is correct for every UX8407AA firmware revision.

At the time of this investigation, no equivalent fix had been identified in
the inspected Linux 7.2-rc4 source or then-current upstream source. The patch
must therefore be described as an out-of-tree candidate, not as an accepted
upstream fix.

## Runtime Changes

The Rust runtime was adjusted after the kernel issue was isolated:

- Startup replay now records ownership when the project itself disables
  `eDP-2` for an attached keyboard.
- Detaching the keyboard may re-enable `eDP-2` only when that disable action
  belongs to the runtime. This preserves the safeguard for a panel that was
  already disabled for another reason.
- Display ownership stores the current kernel boot ID. It survives a daemon
  restart during the same boot but is invalidated after reboot, preventing
  stale state from reopening a display unexpectedly.
- USB keyboard presence polling no longer runs `lsusb` every second. It reads
  `/sys/bus/usb/devices/*/idVendor` and `idProduct` directly for
  `0b05:1cd7`, eliminating repetitive AppArmor denials without changing
  detection behavior.
- Keyboard detach now only ensures Bluetooth is powered. It no longer
  changes Wi-Fi state or implements the obsolete radio-guard behavior.

The runtime library test suite passed 68 tests after these changes.

## Other Project Changes

- Corrected the physical 180-degree mounting baseline of the main panel when
  mapping GNOME orientation state.
- Corrected dual-screen left/right placement and orientation reporting.
- Preserved the selected display scale during orientation changes and added
  a `1.67x` UI option; the default scale is `1.67`.
- Added accelerometer-driven rotation handling in the session agent. This
  snapshot does not yet expose a separate auto-rotation on/off setting in
  the control panel.
- Improved keyboard-backlight state synchronization across attach/detach.
- Added the UX8407AA ASUS WMI quirk project for the firmware events
  `0x5d`, `0x5e`, and `0x5f` that Linux could otherwise interpret as
  RF-kill/airplane-mode keys.
- Reworked application and tray icons from `logo.gif`, including a
  transparent-background tray variant.

See the focused investigation documents in the repository root for the
backlight, rotation, greeter, and earlier eDP-2 A/B evidence.

## Applying the Kernel Patch

From a clean Linux 7.2-rc4 source tree:

```bash
patch -p1 < /path/to/project/patches/kernel/0001-ux8407aa-port-b-tcss-power-and-diagnostics.patch
```

Build with a unique local version so the distribution kernel remains
available as a GRUB fallback. Do not publish kernel source trees, build
directories, `.deb` packages, logs, or crash dumps in this repository; the
root `.gitignore` excludes these local artifacts.

## GitHub Publication Notes

- No password, private key, API token, or environment-secret file was found
  in the project audit performed for this snapshot.
- Machine-local absolute Markdown links were changed to relative paths.
  Historical `/var/log/...` paths remain as evidence labels only;
  the log bundles themselves are not committed.
- `ux8407aa-asus-nb-wmi-quirk` is included as ordinary source under this
  project, not as a Git submodule. Its previous unborn nested Git metadata
  was moved outside the project before publication.
- Keep `Cargo.lock`, `package-lock.json`, source patches, screenshots, and
  documentation under version control. They are intentionally not ignored.
