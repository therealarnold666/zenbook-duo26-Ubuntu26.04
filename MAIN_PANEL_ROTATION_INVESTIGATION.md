# Main Panel Rotation Investigation

## Summary

This document summarizes the currently known facts about the main internal panel
(`eDP-1`, shown as display label `2` in Ubuntu UI) appearing visually rotated
180 degrees on Ubuntu, while touch input remains in the physical orientation.

Observed user-facing result:

- the main panel image is upside down
- touch coordinates remain physically correct
- therefore touch and image appear mismatched by 180 degrees
- a GNOME display configuration workaround was used to force the panel back to
  a visually correct orientation

This issue appears distinct from the `eDP-2` keyboard-attached link failure,
although both happen during the same overall display bring-up period.

## Current Observations

### 1. The issue is not dependent on keyboard-attached vs keyboard-detached boot

Across both recent VBT/ACPI bundles:

- keyboard attached boot:
  - `/var/log/zenbook-duo-vbt-acpi/20260409T053537Z-04459bd4-6016-4f3c-845a-ed8035431bc1`
- keyboard detached boot:
  - `/var/log/zenbook-duo-vbt-acpi/20260409T055829Z-face0c27-c043-482a-a19e-11291c29fca9`

The user reported that the main panel orientation problem exists in both cases.

Therefore:

- the main panel upside-down issue does not currently look conditioned on the
  keyboard-attached boot state in the same way `eDP-2` failure is
- this suggests the main-panel rotation problem and the secondary-panel link
  failure are related in timing, but not necessarily the same trigger

### 2. GNOME is currently carrying an explicit 180-degree correction for `eDP-1`

In both bundles, the captured `monitors.xml` contains:

- `connector = eDP-1`
- `rotation = upside_down`

Files:

- attached boot: `/var/log/zenbook-duo-vbt-acpi/20260409T053537Z-04459bd4-6016-4f3c-845a-ed8035431bc1/monitors.xml`
- detached boot: `/var/log/zenbook-duo-vbt-acpi/20260409T055829Z-face0c27-c043-482a-a19e-11291c29fca9/monitors.xml`

This means the currently running desktop is not showing the raw uncorrected
state. GNOME has already been instructed to compensate by rotating `eDP-1`
upside down.

### 3. The workaround is compensating an earlier wrong baseline, not creating it

User observation indicates:

- without the GNOME config workaround, the main panel image is wrong
- with the workaround, the image is forced back into a usable orientation

Therefore:

- `monitors.xml` is not the original root cause
- it is a compensation layer preserving a usable session state after the real
  orientation error has already occurred earlier in the boot / display bring-up
  path

### 4. The issue appears before normal desktop logic is fully relevant

The user observed that when `xe` is involved, the wrong panel orientation can
already be visible before or around greeter, not only after the full desktop
session is established.

This is important because it weakens the idea that the issue is primarily a
late user-session preference problem.

### 5. The previous non-`xe` observation points lower in the stack

User observation also indicates:

- when `xe` is effectively out of the path, the main panel does not show the
  same upside-down image problem
- when `xe` takes over, the image orientation problem appears

This strongly suggests the incorrect orientation baseline is introduced at or
after `xe` takeover, not by application-level project logic.

## What We Checked In The Bundles

### Checked: `rotation` / `orientation` / `transform` log strings

Searches across:

- `journal-xe-drm-filtered.txt`
- `gnome-shell-journal.txt`
- `mutter-journal.txt`

did not yield a clean explicit statement such as:

- `panel orientation = upside down`
- `VBT panel orientation says ...`
- `Mutter applying transform 180 to eDP-1`

So there is currently no direct one-line proof identifying the exact layer that
first decided the panel should be flipped.

### Checked: DRM logs containing `rotation=1`

Both bundles contain many DRM debug lines with `rotation=1`, including in the
`simple-framebuffer` phase and later `xe` DRM state dumps.

However, this is not currently sufficient proof of the 180-degree issue,
because DRM rotation values are bitmask-based state and the observed log value
alone does not directly map to the user's visible upside-down result in a way we
can safely assert from the collected logs.

Therefore these lines are noted but not treated as decisive evidence.

### Checked: current connector identity

The main panel remains consistently identified as:

- `eDP-1`
- BOE `NB140B9M-T01`

The secondary panel remains:

- `eDP-2`
- BOE `NB140B9M-T02`

So the available evidence does not currently suggest that Linux is simply
swapping panel identities at the connector naming layer.

## Current Best Interpretation

The most likely current interpretation is:

1. The main panel orientation error is introduced before or during the point at
   which `xe` becomes the real DRM/KMS owner.
2. GNOME later persists a compensating 180-degree transform for `eDP-1` in
   `monitors.xml` so the desktop becomes usable.
3. Because touch remains in physical orientation while the image is upside down,
   the visible symptom looks more like a lower-level output orientation problem
   than a clean compositor-side logical rotation.

In short:

- the saved GNOME transform is real
- but it looks more like a workaround for an earlier wrong display baseline than
  the original trigger

## What Is Still Not Proven

The current evidence is not yet sufficient to prove which exact layer first
introduces the wrong main-panel orientation.

The remaining plausible candidates are:

- `xe` / DRM takeover path
- firmware-provided orientation / panel metadata interpreted incorrectly by `xe`
- Mutter / GNOME applying a transform based on incorrect lower-level state

At this point, the data supports suspicion, but not a final attribution.

## Why This Is Separate From The `eDP-2` Attached-Boot Failure

The `eDP-2` investigation showed a strong A/B difference:

- attached boot -> `eDP-2` later degrades and becomes disabled
- detached boot -> `eDP-2` remains enabled

The main-panel orientation issue does not show the same A/B behavior according
to current user observation.

Therefore the two problems should currently be tracked separately:

1. `eDP-2` runtime link instability conditioned by keyboard-attached boot
2. `eDP-1` wrong orientation baseline that GNOME compensates with
   `rotation=upside_down`

## Most Useful Next Step

The next best step for this main-panel problem is a focused live-session capture
of the transform decision itself, ideally without relying only on root-owned
postmortem logs.

That should include:

- current-session `gdbus` / Mutter `DisplayConfig` output for `eDP-1`
- current-session `gdctl show`
- a controlled check with and without the persisted `monitors.xml` compensation
- if possible, a capture of `xe` takeover state immediately before GNOME stores
  the corrective transform

Until that is captured, the best supported statement is:

- the main panel is being corrected by GNOME configuration
- but the underlying incorrect orientation appears to originate earlier than
  ordinary user display preferences
