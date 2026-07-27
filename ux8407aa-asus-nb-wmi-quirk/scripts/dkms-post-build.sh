#!/usr/bin/env bash
set -euo pipefail

# DKMS runs this before it compresses/installs the module, so signatures survive updates.
readonly key_dir='/var/lib/asus-nb-wmi-ux8407aa'
readonly private_key="${key_dir}/MOK.priv"
readonly public_key="${key_dir}/MOK.der"
readonly root_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly module_path="${root_dir}/src/asus-nb-wmi.ko"

if [[ -f "$private_key" && -f "$public_key" ]]; then
	kmodsign sha512 "$private_key" "$public_key" "$module_path"
else
	echo 'No UX8407AA MOK key yet; leaving the DKMS module unsigned.' >&2
fi
