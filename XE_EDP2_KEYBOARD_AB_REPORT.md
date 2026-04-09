# `xe` / `eDP-2` Keyboard A/B Report

## Summary

This document captures the strongest low-level A/B evidence collected so far for
Zenbook Duo UX8407AA on Ubuntu 26.04 with project runtime services disabled.

The key conclusion is:

- With keyboard attached at boot, `xe` successfully discovers `eDP-2` early, but the
  `AUX B / DDI B / PHY B` link later collapses with repeated timeouts and `eDP-2`
  ends up `disabled`.
- With keyboard detached at boot, the same `eDP-2` path initializes and remains
  `enabled`.

This means the root issue is observable below GNOME and below the project
services. It is not explained purely by user display configuration.

## Test Conditions

### Shared Conditions

- Project-related services were disabled/masked for attribution isolation:
  - `zenbook-duo-rust-daemon.service`
  - `zenbook-duo-rust-lifecycle.service`
  - `zenbook-duo-boot-trace.service`
  - `zenbook-duo-session-agent.service`
  - `zenbook-duo-control` autostart disabled
- Kernel debug parameters enabled:
  - `log_buf_len=16M`
  - `ignore_loglevel`
  - `drm.debug=0x1ff`
  - `xe.enable_psr=0`
  - `xe.enable_panel_replay=0`
  - `xe.enable_dpcd_backlight=3`

### Important Interpretation Note

Because project services were disabled, the final visible display layout is not a
reliable indicator of root cause by itself. In particular, even in the
keyboard-detached case, the secondary panel may not remain lit through login and
session transition due to the lack of project-side display management.

For that reason, the A/B comparison focuses on the low-level `xe` / DRM link
state instead of only checking whether the secondary screen looks active to the
user at the end of boot.

## Bundles Compared

### Scenario A: Keyboard Attached At Boot

Bundle:

- `/var/log/zenbook-duo-vbt-acpi/20260409T053537Z-04459bd4-6016-4f3c-845a-ed8035431bc1`

Final DRM sysfs state:

- `card0-eDP-1`: `connected`, `enabled`
- `card0-eDP-2`: `connected`, `disabled`

### Scenario B: Keyboard Detached At Boot

Bundle:

- `/var/log/zenbook-duo-vbt-acpi/20260409T055829Z-face0c27-c043-482a-a19e-11291c29fca9`

Final DRM sysfs state:

- `card0-eDP-1`: `connected`, `enabled`
- `card0-eDP-2`: `connected`, `enabled`

## Shared Early Behavior

In both scenarios, `xe` does all of the following for `eDP-2`:

- chooses `AUX CH B (VBT)`
- adds an `eDP` connector on `DDI B / PHY B`
- reads DPCD / DSC capabilities
- reports `eDP-2` as VRR capable
- configures AUX VESA backlight controls

Representative lines seen in both runs:

- `Using AUX CH B (VBT)`
- `Adding eDP connector on [ENCODER:519:DDI B/PHY B]`
- `[CONNECTOR:520:eDP-2] AUX VESA backlight level is controlled through DPCD`
- `[CONNECTOR:520:eDP-2] VRR capable: yes`

This is important because it shows the `eDP-2` path is not absent from Linux.
The driver initially builds the connector correctly enough to talk to it.

## Divergence Between The Two Scenarios

### Attached-Boot Failure Pattern

In the keyboard-attached run, the `eDP-2` path later degrades into repeated
AUX communication failures on `AUX B / DDI B / PHY B`.

Observed failure signatures:

- repeated `AUX B/DDI B/PHY B: timeout (status 0x7c7c023f)`
- `Too many retries, giving up. First error: -110`
- `*ERROR* Failed to read DPCD register 0x60`

After this degradation:

- `card0-eDP-2/status` remains `connected`
- `card0-eDP-2/enabled` ends up `disabled`

Interpretation:

- The driver initially reaches the panel over AUX.
- The link later becomes unusable at runtime.
- The secondary panel is not merely hidden by GNOME; the low-level path has
  already broken down.

### Detached-Boot Stable Pattern

In the keyboard-detached run, the same `eDP-2` path does not show the attached
run's large `AUX B / DDI B / PHY B` timeout storm.

Observed result:

- `card0-eDP-2/status = connected`
- `card0-eDP-2/enabled = enabled`

Interpretation:

- `xe` can initialize and keep the `eDP-2` path alive when the system boots
  without the keyboard attached.
- This strongly suggests the attached keyboard state is influencing the runtime
  outcome of the `eDP-2` link.

## What This Rules Out

These A/B results significantly weaken or rule out the following as primary
root causes:

- project runtime services triggering secondary-screen shutdown
- GNOME user layout alone (`monitors.xml`) as the first cause
- greeter-only policy decisions as the sole explanation
- a simple "Linux always maps the wrong panel" explanation

The reason is straightforward:

- the difference is already visible in low-level DRM sysfs state
- the difference persists with project services disabled
- the difference is specifically on the `eDP-2 / AUX B / DDI B / PHY B` path

## What The Results Suggest

The most likely class of root cause now is:

- `xe` runtime handling bug involving `eDP-2 / DDI B / PHY B`
- likely conditioned by hardware state present when the keyboard is attached at boot
- possibly influenced by ACPI / WMI / EC / platform state
- not proven to be a pure static VBT parse failure

A more precise current hypothesis is:

- VBT is used to build the `eDP-2` path (`AUX CH B (VBT)` is explicit in logs)
- but the failing behavior is not that Linux cannot discover `eDP-2`
- instead, Linux discovers it, begins normal capability reads, then the runtime
  link later becomes unstable only in the attached-boot scenario

So the issue looks more like:

- `xe` + attached-keyboard hardware/platform state -> unstable `eDP-2` runtime link

than:

- a simple permanent VBT connector identity mix-up

## Related Observation: Main Panel Rotation

Separately observed by the user:

- without `xe`, the main panel orientation appears normal
- after `xe` takeover, the main panel image can appear rotated 180 degrees before greeter
- touch input remains in physical orientation, causing display/input mismatch

This strengthens the case that `xe` takeover is establishing an incorrect or
unstable display baseline before normal desktop logic becomes relevant.

## Recommended Next Step

The next most useful experiment is not further GNOME-level tuning.
It is one of the following:

1. Add even tighter logging around the moment `eDP-2` transitions from normal
   DPCD reads to `AUX B / DDI B / PHY B` timeout failure.
2. Experiment with a pre-greeter workaround that prevents the attached-keyboard
   boot state from influencing the `xe` takeover path.
3. Prepare a minimized upstream-quality bug report for `xe` using the two
   bundles above as paired evidence.
