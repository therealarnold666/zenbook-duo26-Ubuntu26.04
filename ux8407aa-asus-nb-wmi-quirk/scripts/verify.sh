#!/usr/bin/env bash
set -euo pipefail

readonly kernel_release="$(uname -r)"
readonly module_path="$(modinfo -k "$kernel_release" -n asus_nb_wmi)"

printf 'Kernel: %s\n' "$kernel_release"
printf 'DMI: %s / %s\n' "$(cat /sys/class/dmi/id/sys_vendor)" "$(cat /sys/class/dmi/id/product_name)"
printf 'DKMS module: %s\n' "$module_path"
test -f "$module_path"
modinfo "$module_path" | grep -E '^(name|vermagic|signer):'
printf '\nAttach and detach the keyboard, then check that rfkill remains unblocked:\n'
rfkill list
