#!/usr/bin/env bash
set -euo pipefail

PARAMETER="xe.enable_dpcd_backlight=3"
EXPECTED_VENDOR="ASUS"
EXPECTED_PRODUCT="Zenbook Duo UX8407AA"

GRUB_DROPIN="${ZENBOOK_DUO_GRUB_DROPIN:-/etc/default/grub.d/90-zenbook-duo-xe-backlight.cfg}"
DMI_VENDOR_PATH="${ZENBOOK_DUO_DMI_VENDOR_PATH:-/sys/class/dmi/id/sys_vendor}"
DMI_PRODUCT_PATH="${ZENBOOK_DUO_DMI_PRODUCT_PATH:-/sys/class/dmi/id/product_name}"
CMDLINE_PATH="${ZENBOOK_DUO_CMDLINE_PATH:-/proc/cmdline}"

usage() {
    cat <<'EOF'
configure-xe-backlight.sh - manage the UX8407AA xe DPCD backlight parameter

Usage:
  ./tools/configure-xe-backlight.sh install
  ./tools/configure-xe-backlight.sh status
  ./tools/configure-xe-backlight.sh remove

The install action writes a project-owned GRUB drop-in containing:
  xe.enable_dpcd_backlight=3

A reboot is required before a newly installed or removed parameter takes effect.
EOF
}

read_trimmed() {
    tr -d '\r\n' < "$1" 2>/dev/null || true
}

is_supported_machine() {
    [ "$(read_trimmed "${DMI_VENDOR_PATH}")" = "${EXPECTED_VENDOR}" ] &&
        [ "$(read_trimmed "${DMI_PRODUCT_PATH}")" = "${EXPECTED_PRODUCT}" ]
}

as_root() {
    if [ "${EUID}" -eq 0 ]; then
        "$@"
        return
    fi

    if ! command -v sudo >/dev/null 2>&1; then
        echo "ERROR: sudo is required to update GRUB." >&2
        exit 1
    fi
    sudo "$@"
}

resolve_update_grub() {
    if [ -n "${ZENBOOK_DUO_UPDATE_GRUB:-}" ]; then
        printf '%s\n' "${ZENBOOK_DUO_UPDATE_GRUB}"
        return
    fi
    command -v update-grub 2>/dev/null || true
}

render_dropin() {
    cat <<'EOF'
# Managed by zenbook-duo26-Ubuntu26.04.
# Force the Intel DPCD backlight interface used by the UX8407AA eDP panels.
_zenbook_duo_cmdline=""
for _zenbook_duo_arg in ${GRUB_CMDLINE_LINUX_DEFAULT:-}; do
    case "${_zenbook_duo_arg}" in
        xe.enable_dpcd_backlight=*)
            ;;
        *)
            _zenbook_duo_cmdline="${_zenbook_duo_cmdline}${_zenbook_duo_cmdline:+ }${_zenbook_duo_arg}"
            ;;
    esac
done
GRUB_CMDLINE_LINUX_DEFAULT="${_zenbook_duo_cmdline}${_zenbook_duo_cmdline:+ }xe.enable_dpcd_backlight=3"
unset _zenbook_duo_arg _zenbook_duo_cmdline
EOF
}

update_grub() {
    local update_command
    update_command="$(resolve_update_grub)"
    if [ -z "${update_command}" ]; then
        echo "ERROR: update-grub was not found; GRUB was not regenerated." >&2
        return 1
    fi
    as_root "${update_command}"
}

install_parameter() {
    if ! is_supported_machine; then
        echo "Skipping xe backlight parameter: this machine is not ${EXPECTED_PRODUCT}."
        return
    fi

    local temp_file
    temp_file="$(mktemp)"
    render_dropin > "${temp_file}"

    as_root install -D -m 0644 "${temp_file}" "${GRUB_DROPIN}"
    rm -f "${temp_file}"
    update_grub

    echo "Configured ${PARAMETER} in ${GRUB_DROPIN}."
    if parameter_is_active; then
        echo "The parameter is already active in the running kernel."
    else
        echo "Reboot once to activate the parameter."
    fi
}

remove_parameter() {
    if [ -e "${GRUB_DROPIN}" ]; then
        as_root rm -f "${GRUB_DROPIN}"
        update_grub
        echo "Removed the project-managed ${PARAMETER} GRUB drop-in."
    else
        echo "No project-managed xe backlight GRUB drop-in is installed."
    fi

    if parameter_is_active; then
        echo "Reboot once to remove the parameter from the running kernel."
    fi
}

parameter_is_active() {
    [ -r "${CMDLINE_PATH}" ] &&
        grep -Fqw -- "${PARAMETER}" "${CMDLINE_PATH}"
}

show_status() {
    local configured="no"
    local active="no"

    if [ -r "${GRUB_DROPIN}" ] &&
        grep -Fq -- "${PARAMETER}" "${GRUB_DROPIN}"; then
        configured="yes"
    fi
    if parameter_is_active; then
        active="yes"
    fi

    printf 'machine-supported=%s\n' "$(is_supported_machine && echo yes || echo no)"
    printf 'grub-configured=%s\n' "${configured}"
    printf 'running-kernel-active=%s\n' "${active}"
    printf 'parameter=%s\n' "${PARAMETER}"
}

case "${1:-status}" in
    install)
        install_parameter
        ;;
    status)
        show_status
        ;;
    remove)
        remove_parameter
        ;;
    -h|--help|help)
        usage
        ;;
    *)
        usage >&2
        exit 2
        ;;
esac
