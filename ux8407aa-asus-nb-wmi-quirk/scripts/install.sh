#!/usr/bin/env bash
set -euo pipefail

readonly package='asus-nb-wmi-ux8407aa'
readonly version='0.1.0'
readonly root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly kernel_release="$(uname -r)"

if [[ "$(cat /sys/class/dmi/id/sys_vendor)" != 'ASUS' || \
      "$(cat /sys/class/dmi/id/product_name)" != 'Zenbook Duo UX8407AA' ]]; then
	echo 'Refusing to install: this DKMS quirk is only for ASUS Zenbook Duo UX8407AA.' >&2
	exit 1
fi

sudo apt-get update
sudo apt-get install --yes dkms "linux-headers-${kernel_release}"
sudo install -d "/usr/src/${package}-${version}"
sudo cp -a --no-preserve=ownership "${root_dir}/." "/usr/src/${package}-${version}/"
sudo dkms add -m "$package" -v "$version" 2>/dev/null || true
sudo dkms build -m "$package" -v "$version" -k "$kernel_release"
sudo dkms install --force -m "$package" -v "$version" -k "$kernel_release"
sudo depmod -a "$kernel_release"

echo 'DKMS module installed. Run scripts/enroll-mok.sh before rebooting when Secure Boot is enabled.'
