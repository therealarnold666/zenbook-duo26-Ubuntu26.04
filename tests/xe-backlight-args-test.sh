#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPT="${ROOT_DIR}/tools/configure-xe-backlight.sh"
TEMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TEMP_DIR}"' EXIT

mkdir -p "${TEMP_DIR}/bin"
printf 'ASUS\n' > "${TEMP_DIR}/vendor"
printf 'Zenbook Duo UX8407AA\n' > "${TEMP_DIR}/product"
printf 'quiet splash\n' > "${TEMP_DIR}/cmdline"

cat > "${TEMP_DIR}/bin/sudo" <<'EOF'
#!/usr/bin/env bash
exec "$@"
EOF

cat > "${TEMP_DIR}/bin/update-grub" <<'EOF'
#!/usr/bin/env bash
printf 'updated\n' >> "${UPDATE_LOG}"
EOF

chmod +x "${TEMP_DIR}/bin/sudo" "${TEMP_DIR}/bin/update-grub"
export PATH="${TEMP_DIR}/bin:/usr/bin:/bin"
export UPDATE_LOG="${TEMP_DIR}/update.log"
export ZENBOOK_DUO_GRUB_DROPIN="${TEMP_DIR}/grub.d/90-zenbook-duo-xe-backlight.cfg"
export ZENBOOK_DUO_DMI_VENDOR_PATH="${TEMP_DIR}/vendor"
export ZENBOOK_DUO_DMI_PRODUCT_PATH="${TEMP_DIR}/product"
export ZENBOOK_DUO_CMDLINE_PATH="${TEMP_DIR}/cmdline"
export ZENBOOK_DUO_UPDATE_GRUB="${TEMP_DIR}/bin/update-grub"

"${SCRIPT}" install >/dev/null
test -f "${ZENBOOK_DUO_GRUB_DROPIN}"
grep -q 'xe.enable_dpcd_backlight=3' "${ZENBOOK_DUO_GRUB_DROPIN}"
test "$(wc -l < "${UPDATE_LOG}")" -eq 1

GRUB_CMDLINE_LINUX_DEFAULT="quiet xe.enable_dpcd_backlight=1 splash"
# shellcheck disable=SC1090
. "${ZENBOOK_DUO_GRUB_DROPIN}"
test "${GRUB_CMDLINE_LINUX_DEFAULT}" = "quiet splash xe.enable_dpcd_backlight=3"

status="$("${SCRIPT}" status)"
grep -q '^machine-supported=yes$' <<< "${status}"
grep -q '^grub-configured=yes$' <<< "${status}"
grep -q '^running-kernel-active=no$' <<< "${status}"

printf 'quiet splash xe.enable_dpcd_backlight=3\n' > "${TEMP_DIR}/cmdline"
grep -q '^running-kernel-active=yes$' < <("${SCRIPT}" status)

"${SCRIPT}" remove >/dev/null
test ! -e "${ZENBOOK_DUO_GRUB_DROPIN}"
test "$(wc -l < "${UPDATE_LOG}")" -eq 2

echo "PASS"
