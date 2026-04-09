# Post-Login Display Recovery Findings

## Summary

This document summarizes what we learned from the independent `debug/post-login-display-recovery/` experiment.

The experiment goal was not to fix `xe` at the driver level. The goal was to test whether a delayed desktop-side recovery step could detect a bad display state after login and restore a usable dual-screen layout.

Current result:

- The first recovery implementation was too aggressive and caused extra disruption.
- The second recovery implementation avoided unnecessary actions on healthy desktops.
- However, the second implementation still failed to detect one important failure mode:
  - `eDP-2` is logically present and `enabled`
  - GNOME still reports a dual-monitor layout
  - but the secondary screen is physically dark / not actually rendering
  - while kernel pipe-B errors are present

So the current recovery experiment is not yet sufficient as a workaround.

## Key Runs

### Run A: Aggressive False Positive Recovery

Recovery run:

- `/var/log/zenbook-duo-display-recovery/20260409T090135Z-38cfbcb0-2dc1-402b-8ed7-660259c2acd3`

What happened:

- `health-before.txt` already showed a logically healthy state:
  - `eDP-1 = connected + enabled`
  - `eDP-2 = connected + enabled`
  - `gdctl_ok = yes`
  - `logical_edp1 = yes`
  - `logical_edp2 = yes`
  - `primary_transform = 180`
- But recovery still triggered because the health model treated recent kernel display errors alone as sufficient reason to recover.
- The recovery service then issued three display rebuild attempts using `gdctl set`.
- User-observed result:
  - multiple stalls after login
  - GNOME / GJS responsiveness problems

Important log:

- [recover.log](/var/log/zenbook-duo-display-recovery/20260409T090135Z-38cfbcb0-2dc1-402b-8ed7-660259c2acd3/recover.log)

Main conclusion from this run:

- A recovery service must not trigger solely because earlier kernel errors exist.
- If the current desktop topology is already stable, forcing repeated `gdctl set` calls can make GNOME responsiveness worse.

### Run B: Conservative Health Check

Recovery run:

- `/var/log/zenbook-duo-display-recovery/20260409T092330Z-57cfabc7-17a5-4f71-8665-3d1e936d133e`

What happened:

- `health-before.txt` reported:
  - `eDP-1 = connected + enabled`
  - `eDP-2 = connected + enabled`
  - `gdctl_ok = yes`
  - `logical_edp1 = yes`
  - `logical_edp2 = yes`
  - `primary_transform = 180`
  - `recovery_needed = no`
- `recover.log` then correctly exited without applying any recovery command:
  - `display already healthy; no recovery action needed`

Important logs:

- [health-before.txt](/var/log/zenbook-duo-display-recovery/20260409T092330Z-57cfabc7-17a5-4f71-8665-3d1e936d133e/health-before.txt)
- [recover.log](/var/log/zenbook-duo-display-recovery/20260409T092330Z-57cfabc7-17a5-4f71-8665-3d1e936d133e/recover.log)
- [health-after.txt](/var/log/zenbook-duo-display-recovery/20260409T092330Z-57cfabc7-17a5-4f71-8665-3d1e936d133e/health-after.txt)

But the user-observed state was still bad:

- secondary display remained physically dark after login

At the same time, kernel errors were still present:

- [kernel-display-errors.txt](/var/log/zenbook-duo-display-recovery/20260409T092330Z-57cfabc7-17a5-4f71-8665-3d1e936d133e/kernel-display-errors.txt)
  - `flip_done timed out`
  - `DSB 0 timed out waiting for idle`

Main conclusion from this run:

- The current health model can produce a **false healthy** result.
- Logical topology and connector state are not enough to prove that the secondary screen is truly rendering.

## Important Technical Finding

The recovery experiment exposed a critical distinction:

- **logical health**
  - connectors exist
  - `enabled` is true
  - GNOME reports two logical monitors
  - transforms look correct

is not the same as:

- **physical rendering health**
  - the lower screen is actually lit and usable
  - pipe B is not stuck

On this machine, these can diverge.

The strongest signal for that divergence is:

- `eDP-2 = enabled`
- while kernel logs still contain pipe-B failures such as:
  - `flip_done timed out`
  - `DSB 0 timed out waiting for idle`

That means the current recovery checker is still too shallow.

## Recovery Baseline Used

The experiment uses the current project GNOME detached baseline:

- primary: `eDP-1`
- secondary: `eDP-2`
- primary transform: `180`
- secondary transform: `normal`
- secondary position: `below eDP-1`
- scale: `1.6666666269302368`

This baseline is currently encoded in:

- [display-recover.sh](/home/arnold/Projects/zenbook-duo-linux-main/debug/post-login-display-recovery/display-recover.sh)

## Problems Identified In The Recovery Design

### 1. First version: false positive trigger

The first version treated historical kernel display errors as sufficient reason to rebuild the display layout even when the current layout was already healthy.

Impact:

- extra `gdctl set` calls
- repeated desktop stutter
- possible GNOME / GJS instability

### 2. Second version: false healthy classification

The second version fixed the false positive trigger, but then failed to catch the case where:

- topology is logically correct
- `eDP-2` is enabled
- but the secondary screen is still dark

Impact:

- recovery never runs in one of the most important real-world bad states

### 3. `gdctl show` itself is not a perfect health oracle

In earlier runs, `gdctl show` sometimes crashed while printing backlight preferences even though it had already emitted useful monitor and logical-monitor information.

That means:

- `gdctl` output should be parsed defensively
- `gdctl` success/failure alone cannot be treated as a complete truth source

## Current Interpretation

The post-login recovery direction still looks promising, but only if both pieces are improved:

1. **health detection**
   - must recognize the "fake healthy" state
   - should consider pipe-B failure signals as real evidence when connector/layout state still looks normal

2. **recovery strategy**
   - a single direct dual-screen rebuild may not be enough
   - a better next experiment is likely:
     - step 1: force a known-good single-screen state
     - step 2: wait briefly
     - step 3: re-add `eDP-2`

## Practical Conclusion

At the current stage:

- the recovery experiment is useful for diagnosis
- it is not yet a reliable workaround

What it already proved:

- desktop-side recovery should not fire on healthy layouts
- logical dual-monitor presence does not guarantee a physically working secondary panel
- kernel pipe-B errors remain the most important clue when the lower screen is dark despite `eDP-2` still appearing enabled
