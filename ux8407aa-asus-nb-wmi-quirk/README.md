# ASUS Zenbook Duo UX8407AA rfkill quirk backport

This project backports Linux upstream commit `2997606dd17729404cef9821ce66dd037b6019eb` to the Ubuntu 7.0 kernel series. It adds the UX8407AA DMI match to the existing Zenbook Duo keyboard quirk in `asus_nb_wmi`, causing the spurious WMI events `0x5D`, `0x5E`, and `0x5F` to be ignored instead of reported as wireless/rfkill keys.

The source baseline is Linux v7.0. The only functional delta is recorded in `patches/0001-asus-nb-wmi-add-ux8407aa-dmi-quirk.patch` and already applied to `src/asus-nb-wmi.c`.

## Install

From the parent project directory, enter this project first:

```bash
cd ~/Projects/zenbook-duo26-Ubuntu26.04/ux8407aa-asus-nb-wmi-quirk
./scripts/install.sh
./scripts/enroll-mok.sh
sudo reboot
```

`install.sh` validates the local DMI strings, installs DKMS and matching headers if required, and installs an `asus-nb-wmi` override in `/lib/modules/<kernel>/updates/dkms`. DKMS rebuilds and signs it for later kernels.

Secure Boot is enabled on this machine. Ubuntu DKMS signs modules with its certificate at `/var/lib/shim-signed/mok/MOK.der`; `enroll-mok.sh` registers that certificate with MOK. At the next boot, select **Enroll MOK**, enter the one-time password, and reboot once more if prompted.

## Verify and rollback

After booting, run:

```bash
./scripts/verify.sh
```

Attach and detach the keyboard and confirm `rfkill list` leaves Wi-Fi and Bluetooth unblocked. To remove the backport, run `./scripts/uninstall.sh` and reboot.

## Scope

The driver entry is limited to DMI vendor `ASUS` and product name `Zenbook Duo UX8407AA`; no other machines match it. Once an Ubuntu kernel contains this upstream commit, remove this DKMS package to return to the distribution module.
