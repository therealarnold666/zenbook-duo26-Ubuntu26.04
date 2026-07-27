#!/usr/bin/env bash
set -euo pipefail

readonly public_key='/var/lib/shim-signed/mok/MOK.der'

if ! mokutil --sb-state 2>/dev/null | grep -qi 'SecureBoot enabled'; then
	echo 'Secure Boot is disabled; no MOK enrollment is needed.'
	exit 0
fi

if [[ ! -f "$public_key" ]]; then
	echo 'Missing the Ubuntu DKMS signing certificate. Run scripts/install.sh first.' >&2
	exit 1
fi

sudo mokutil --import "$public_key"
echo 'Enroll the displayed MOK password in the blue MOK Manager screen after rebooting.'
