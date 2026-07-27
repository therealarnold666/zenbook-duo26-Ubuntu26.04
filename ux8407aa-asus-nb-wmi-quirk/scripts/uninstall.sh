#!/usr/bin/env bash
set -euo pipefail

readonly package='asus-nb-wmi-ux8407aa'
readonly version='0.1.0'
readonly kernel_release="$(uname -r)"

sudo dkms remove -m "$package" -v "$version" --all || true
sudo depmod -a "$kernel_release"
echo 'DKMS override removed. Reboot, or unload and reload asus_nb_wmi, to use the distribution module.'
