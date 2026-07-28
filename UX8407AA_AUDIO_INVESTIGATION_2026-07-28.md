# UX8407AA Ubuntu Audio Investigation

Date: 2026-07-28

## Symptom

Windows audio works, but Ubuntu exposes no speakers, microphone, or wired
headset. PipeWire shows only `Dummy Output`, `aplay -l` reports no soundcards,
and `/proc/asound/cards` is empty.

## Root Cause: Kernel Card Registration

The PCI audio controller and all real codecs probe far enough to identify:

- two CS35L56 amplifiers on SoundWire link 2;
- one CS42L43 codec on SoundWire link 3;
- one RT722 entry on SoundWire link 3.

The RT722 entry is a firmware ghost. ASUS's UX8407AA Windows audio package
identifies CS42L43 as the machine codec, and the laptop has one physical
3.5 mm combo jack. The same ghost RT722 firmware pattern is already handled
by Linux for other ASUS Panther Lake systems.

Without a UX8407AA DMI match, both CS42L43 and RT722 advertise a Jack function.
The generic `sof_sdw` machine driver creates two runtime devices named:

```text
SDW3-Playback-SimpleJack
```

The second sysfs registration fails with `-EEXIST`. This aborts the entire
ASoC card probe, so the failure appears in userspace as a missing soundcard
rather than a routing or mute problem.

Representative kernel messages:

```text
No SoundWire machine driver found for the ACPI-reported configuration
link 3 mfg_id 0x01fa part_id 0x4243 version 0x3
link 3 mfg_id 0x025d part_id 0x0722 version 0x3
sysfs: cannot create duplicate filename '.../SDW3-Playback-SimpleJack'
sof_sdw: probe with driver sof_sdw failed with error -12
```

## Kernel Fix

`patches/kernel/0002-ux8407aa-ignore-ghost-rt722.patch` adds UX8407AA to
the existing SoundWire `ghost_realtek` DMI table. It remaps the phantom
RT722 address `0x000330025d072201` to zero before the machine driver builds
the card.

This is intentionally narrower than renaming duplicate DAI links or disabling
function topologies. Those alternatives would retain a non-existent codec,
could bind the same topology pipeline twice, and could produce duplicate jack
controls later in card initialization.

## Build And Test

For the current custom 7.2-rc4 kernel, build and install only the matching
SoundWire Intel module:

```bash
./tools/build-audio-quirk.sh /path/to/linux-7.2-rc4
sudo ./tools/build-audio-quirk.sh install /path/to/linux-7.2-rc4
sudo reboot
```

The test machine has Secure Boot enabled and an existing enrolled Ubuntu DKMS
MOK. The install action signs only the installed module copy with that
root-protected key before rebuilding the initramfs.

Verify after reboot:

```bash
aplay -l
wpctl status
journalctl -b -k | rg -i 'soundwire|sof_sdw|cs42l43|cs35l56|rt722'
```

Expected kernel results:

- no RT722 enumeration;
- no duplicate `SDW3-Playback-SimpleJack`;
- a SOF SoundWire ALSA card exists.

## Root Cause: Userspace Routing

Card registration exposed a second, independent compatibility gap. The kernel
reports:

```text
spk:cs35l56+cs42l43-spk hs:cs42l43 mic:cs42l43-dmic
```

Ubuntu 26.04's `alsa-ucm-conf` speaker regex truncates that value to
`cs35l56+cs42l43`. ALSA then tries to import the nonexistent
`/usr/share/alsa/ucm2/codecs/cs35l56+cs42l43/init.conf`. WirePlumber falls
back to ALSA device 0 (`Jack Out`) instead of device 2 (`Speaker`).

ALSA UCM upstream fixed this on 2026-04-15 in commits
`dd191521cb1d553c3e670323c97c8a6052d0b861` and
`980fb83651e82c3e53d3a0ab7fa9b7d6fc2d809b`. This project backports only
the relevant CS42L43 and CS35L56 configuration:

```bash
./tools/configure-audio-ucm.sh validate
sudo ./tools/configure-audio-ucm.sh install
./tools/configure-audio-ucm.sh activate
```

The `activate` action restarts the user audio services and generates one mute
state edge. This synchronizes hardware switches that may retain the old,
pre-fix state while WirePlumber already believes the sink is unmuted.

Expected userspace results:

- `wpctl status` exposes `sof-soundwire Speaker` and `Microphones`;
- Speaker uses the HiFi UCM profile and ALSA playback device 2;
- CS42L43 inputs select `DP6RX1` and `DP6RX2`;
- the CS42L43 digital switch and both CS35L56 amplifier switches are on while
  the speaker route is active.

The kernel still logs missing optional CS35L56 model-specific tuning and
calibration controls. Do not alias firmware from another laptop: incorrect
speaker calibration can be unsafe. This remains a separate follow-up only if
audio is still silent after the verified UCM route is active.

## Rollback

The override is installed under `/lib/modules/<kernel>/updates/zenbook-duo/`;
the original in-tree module is not deleted.

```bash
sudo ./tools/configure-audio-ucm.sh remove
sudo ./tools/build-audio-quirk.sh remove
sudo reboot
```
