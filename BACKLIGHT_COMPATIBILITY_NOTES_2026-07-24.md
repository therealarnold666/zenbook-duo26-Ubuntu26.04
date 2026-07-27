# Backlight Compatibility Notes

Date: 2026-07-24

This note records the current backlight layout on the local machine and compares it with the paths hardcoded in this project.

## Current resolution

The current runtime no longer relies on only `card1-eDP-2-backlight`. It
checks `card0-eDP-2-backlight`, `card1-eDP-2-backlight`, and
`asus_screenpad` in that order. The setup scripts grant access to both DRM
backlight names.

The UX8407AA installer also manages the xe startup parameter
`xe.enable_dpcd_backlight=3` through
`tools/configure-xe-backlight.sh`. This forces the Intel DPCD backlight
interface selected during the earlier A/B investigation. It is independent
from the PSR and Panel Replay stability parameters.

The remainder of this document preserves the original compatibility snapshot.

## Local machine

Machine:

- Model: `Zenbook Duo UX8407AA`
- OS: `Ubuntu 26.04 LTS`
- Session: `GNOME on Wayland`
- Kernel: `7.0.0-28-generic`

Current `/sys/class/backlight` entries:

```text
/sys/class/backlight/asus_screenpad
/sys/class/backlight/card0-eDP-2-backlight
/sys/class/backlight/intel_backlight
```

Resolved device paths:

```text
/sys/class/backlight/asus_screenpad
  -> /sys/devices/platform/asus-nb-wmi/backlight/asus_screenpad

/sys/class/backlight/card0-eDP-2-backlight
  -> /sys/devices/pci0000:00/0000:00:02.0/drm/card0/card0-eDP-2/card0-eDP-2-backlight

/sys/class/backlight/intel_backlight
  -> /sys/devices/pci0000:00/0000:00:02.0/drm/card0/card0-eDP-1/intel_backlight
```

Observed values at capture time:

```text
== /sys/class/backlight/asus_screenpad ==
brightness=130898
max_brightness=255
actual_brightness=82
type=raw

== /sys/class/backlight/card0-eDP-2-backlight ==
brightness=1920
max_brightness=38400
actual_brightness=1920
type=raw

== /sys/class/backlight/intel_backlight ==
brightness=2021
max_brightness=38400
actual_brightness=2021
type=raw
```

## Project assumptions

The project consistently assumes:

- Primary/main panel backlight: `/sys/class/backlight/intel_backlight`
- Secondary/bottom panel backlight: `/sys/class/backlight/card1-eDP-2-backlight`

This is only a partial match for the current machine.

## Direct comparison

Expected by project:

```text
Primary:   /sys/class/backlight/intel_backlight
Secondary: /sys/class/backlight/card1-eDP-2-backlight
```

Observed on this machine:

```text
Primary-like node:   /sys/class/backlight/intel_backlight
Secondary-like node: /sys/class/backlight/card0-eDP-2-backlight
Extra ASUS node:     /sys/class/backlight/asus_screenpad
```

Main mismatch:

- The project expects `card1-eDP-2-backlight`
- The machine currently exposes `card0-eDP-2-backlight`

## References in this repository

Installer scripts add sudoers entries for:

- [setup-gnome.sh](setup-gnome.sh)
- [setup-kde.sh](setup-kde.sh)
- [setup-niri.sh](setup-niri.sh)

Those entries include:

```text
/sys/class/backlight/card1-eDP-2-backlight/brightness
/sys/class/backlight/intel_backlight/brightness
```

Uninstall cleanup also assumes the same secondary path:

- [uninstall.sh](uninstall.sh)

Runtime code references:

- [ui-tauri-react/src-tauri/src/runtime/session_agent.rs](ui-tauri-react/src-tauri/src/runtime/session_agent.rs)
- [ui-tauri-react/src-tauri/src/usb_media_remap_helper.rs](ui-tauri-react/src-tauri/src/usb_media_remap_helper.rs)
- [ui-tauri-react/src-tauri/src/hardware/sysfs.rs](ui-tauri-react/src-tauri/src/hardware/sysfs.rs)
- [ui-tauri-react/src-tauri/src/watchers/file_watcher.rs](ui-tauri-react/src-tauri/src/watchers/file_watcher.rs)

## Likely meaning of the local nodes

Based on the naming and DRM path layout:

- `intel_backlight` appears to map to `card0-eDP-1`, likely the main internal panel
- `card0-eDP-2-backlight` appears to map to `card0-eDP-2`, likely the second internal panel
- `asus_screenpad` is an additional ASUS WMI-managed brightness node and should not be assumed to be a drop-in replacement for the secondary DRM backlight path without explicit validation

This mapping is an inference from the sysfs names and device paths.

## Impact on compatibility

What should still work:

- Primary brightness reads and writes that use `intel_backlight`
- Any logic that only watches `intel_backlight`

What is at risk:

- Secondary brightness mirroring in the session agent
- Secondary brightness writes from helper binaries
- Passwordless sudo brightness sync for the secondary panel
- Uninstall cleanup consistency if the install logic is changed later without updating the uninstall script

What may happen in practice:

- Secondary brightness sync silently does nothing because the hardcoded path does not exist
- The installer writes a sudoers rule for a non-existent backlight node
- The bottom panel may remain out of sync with the main panel brightness even if the rest of the dock/undock logic works

## Summary

The local machine is close to the project's expected layout, but not identical.

The key incompatibility is:

```text
project secondary path: /sys/class/backlight/card1-eDP-2-backlight
local secondary path:   /sys/class/backlight/card0-eDP-2-backlight
```

Before installation or before trusting brightness-related features, the project should be adjusted to detect the secondary backlight path dynamically instead of hardcoding `card1-eDP-2-backlight`.
