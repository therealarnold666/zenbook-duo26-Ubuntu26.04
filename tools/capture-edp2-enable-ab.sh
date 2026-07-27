#!/usr/bin/env bash
set -uo pipefail

readonly SCALE="1.6666666269302368"
readonly MODE="2880x1800@60.000"
readonly STAMP="$(date +%Y%m%d-%H%M%S)"
readonly OUT="${HOME}/zenbook-xe-captures/edp2-enable-${STAMP}"

mkdir -p "${OUT}"

capture_user_state() {
    local suffix="$1"

    gdctl show --verbose >"${OUT}/gdctl-${suffix}.txt" 2>&1 || true
    {
        printf 'kernel: '
        uname -r
        printf 'cmdline: '
        cat /proc/cmdline
        printf 'time: '
        date --iso-8601=seconds
    } >"${OUT}/system-${suffix}.txt"

    for connector in /sys/class/drm/card*-eDP-*; do
        [ -d "${connector}" ] || continue
        {
            printf 'connector=%s\n' "${connector}"
            printf 'status='
            cat "${connector}/status" 2>/dev/null || true
            printf 'enabled='
            cat "${connector}/enabled" 2>/dev/null || true
            printf 'dpms='
            cat "${connector}/dpms" 2>/dev/null || true
        } >>"${OUT}/connectors-${suffix}.txt"
    done
}

capture_root_state() {
    local out="$1"
    local suffix="$2"
    local dri

    journalctl -b -k --no-pager >"${out}/kernel-${suffix}.log" 2>&1 || true
    journalctl -b --no-pager \
        _COMM=gnome-shell >"${out}/gnome-shell-${suffix}.log" 2>&1 || true

    for dri in /sys/kernel/debug/dri/*; do
        [ -d "${dri}" ] || continue
        if [ -r "${dri}/state" ]; then
            cat "${dri}/state" >"${out}/drm-state-${suffix}-$(basename "${dri}").txt"
        fi
        if [ -r "${dri}/i915_display_info" ]; then
            cat "${dri}/i915_display_info" \
                >"${out}/display-info-${suffix}-$(basename "${dri}").txt"
        fi
    done
}

echo "Capture directory: ${OUT}"
echo "Preparing privileged DRM capture (sudo may ask for your password)..."
sudo -v

capture_user_state before
sudo bash -c "$(declare -f capture_root_state); capture_root_state \"\$1\" before" \
    _ "${OUT}"

echo "Enabling eDP-2 once at 60 Hz without changing monitors.xml..."
set +e
timeout 25s gdctl set \
    --logical-monitor --monitor eDP-1 --mode "${MODE}" \
        --primary --scale "${SCALE}" --transform 180 --x 0 --y 0 \
    --logical-monitor --monitor eDP-2 --mode "${MODE}" \
        --scale "${SCALE}" --transform normal --below eDP-1 \
    >"${OUT}/gdctl-set.txt" 2>&1
gdctl_status=$?
set -e
printf '%s\n' "${gdctl_status}" >"${OUT}/gdctl-set.exit-status"

echo "Watching the first 35 seconds after the display commit..."
for second in 1 3 8 15 25 35; do
    sleep "$((second - ${previous_second:-0}))"
    capture_user_state "after-${second}s"
    previous_second="${second}"
done

sudo bash -c "$(declare -f capture_root_state); capture_root_state \"\$1\" after-35s" \
    _ "${OUT}"

journalctl -b -k --since "@$(stat -c %Y "${OUT}/system-before.txt")" --no-pager \
    | grep -Ei 'xe|drm|flip_done|DSB|AUX|PHY|DPLL|link training|vblank|pipe [AB]' \
    >"${OUT}/xe-relevant.log" 2>&1 || true

tar -C "$(dirname "${OUT}")" -czf "${OUT}.tar.gz" "$(basename "${OUT}")"

echo
echo "Capture complete: ${OUT}.tar.gz"
echo "gdctl exit status: ${gdctl_status}"
echo "The persistent single-screen monitors.xml was not changed."
