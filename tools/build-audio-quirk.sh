#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
PATCH_FILE="$ROOT_DIR/patches/kernel/0002-ux8407aa-ignore-ghost-rt722.patch"
KERNEL_RELEASE="${KERNEL_RELEASE:-$(uname -r)}"
KERNEL_SOURCE="${1:-}"
BUILD_DIR="${BUILD_DIR:-}"
MODULE_RELATIVE="drivers/soundwire/soundwire-intel.ko"
INSTALL_RELATIVE="updates/zenbook-duo/soundwire-intel.ko"
MOK_PRIVATE_KEY="${MOK_PRIVATE_KEY:-/var/lib/shim-signed/mok/MOK.priv}"
MOK_CERTIFICATE="${MOK_CERTIFICATE:-/var/lib/shim-signed/mok/MOK.der}"

usage() {
  cat <<'EOF'
Usage:
  ./tools/build-audio-quirk.sh /path/to/linux-7.2-rc4
  sudo ./tools/build-audio-quirk.sh install /path/to/linux-7.2-rc4
  sudo ./tools/build-audio-quirk.sh remove

The source tree must match the running kernel base. BUILD_DIR may point to a
separate prepared output directory. Installation adds an override module under
/lib/modules/<kernel>/updates and leaves the original in-tree module intact.
EOF
}

apply_patch_once() {
  if rg -q 'DMI_MATCH\\(DMI_BOARD_NAME, "UX8407AA"\\)' \
      "$KERNEL_SOURCE/drivers/soundwire/dmi-quirks.c"; then
    return
  fi

  patch -d "$KERNEL_SOURCE" -p1 < "$PATCH_FILE"
}

prepare_tree() {
  local headers="/lib/modules/$KERNEL_RELEASE/build"
  local config="/boot/config-$KERNEL_RELEASE"

  [[ -d "$KERNEL_SOURCE" ]] || {
    echo "ERROR: Kernel source tree not found: $KERNEL_SOURCE" >&2
    exit 1
  }
  [[ -d "$headers" && -f "$headers/Module.symvers" ]] || {
    echo "ERROR: Matching headers are not installed for $KERNEL_RELEASE" >&2
    exit 1
  }
  [[ -f "$config" ]] || {
    echo "ERROR: Missing kernel config: $config" >&2
    exit 1
  }

  apply_patch_once
  mkdir -p "$BUILD_DIR"
  cp "$config" "$BUILD_DIR/.config"
  make -s -C "$KERNEL_SOURCE" O="$BUILD_DIR" olddefconfig
  make -s -C "$KERNEL_SOURCE" O="$BUILD_DIR" prepare modules_prepare
  cp "$headers/Module.symvers" "$BUILD_DIR/Module.symvers"

  local built_release
  built_release="$(make -s -C "$KERNEL_SOURCE" O="$BUILD_DIR" kernelrelease)"
  if [[ "$built_release" != "$KERNEL_RELEASE" ]]; then
    echo "ERROR: Source/config release is $built_release, expected $KERNEL_RELEASE" >&2
    exit 1
  fi
}

build_module() {
  prepare_tree
  make -s -C "$KERNEL_SOURCE" O="$BUILD_DIR" M=drivers/soundwire soundwire-intel.ko

  local module="$BUILD_DIR/$MODULE_RELATIVE"
  [[ -f "$module" ]] || {
    echo "ERROR: Build did not produce $module" >&2
    exit 1
  }

  validate_module "$module"
  echo "Built: $module"
}

validate_module() {
  local module="$1"
  local vermagic

  vermagic="$(modinfo -F vermagic "$module" | awk '{print $1}')"
  if [[ "$vermagic" != "$KERNEL_RELEASE" ]]; then
    echo "ERROR: Module vermagic is $vermagic, expected $KERNEL_RELEASE" >&2
    exit 1
  fi
}

sign_for_secure_boot() {
  local module="$1"
  local sign_file="/lib/modules/$KERNEL_RELEASE/build/scripts/sign-file"
  local secure_boot_state

  secure_boot_state="$(mokutil --sb-state 2>/dev/null || true)"
  if ! rg -q 'SecureBoot enabled' <<< "$secure_boot_state"; then
    return
  fi

  [[ -x "$sign_file" ]] || {
    echo "ERROR: Kernel module signing tool is missing: $sign_file" >&2
    return 1
  }
  [[ -r "$MOK_PRIVATE_KEY" && -r "$MOK_CERTIFICATE" ]] || {
    echo "ERROR: Secure Boot is enabled but the enrolled MOK key is unavailable" >&2
    return 1
  }

  local certificate_fingerprint
  local enrolled_certificates

  certificate_fingerprint="$(
    openssl x509 -inform DER -in "$MOK_CERTIFICATE" -noout -fingerprint -sha1 |
      cut -d= -f2 |
      tr '[:upper:]' '[:lower:]'
  )"
  enrolled_certificates="$(
    mokutil --list-enrolled 2>/dev/null |
      tr '[:upper:]' '[:lower:]'
  )"
  if ! rg -Fq "$certificate_fingerprint" <<< "$enrolled_certificates"; then
    echo "ERROR: $MOK_CERTIFICATE is not enrolled in Secure Boot" >&2
    return 1
  fi

  "$sign_file" sha512 "$MOK_PRIVATE_KEY" "$MOK_CERTIFICATE" "$module"
  [[ -n "$(modinfo -F signer "$module")" ]] || {
    echo "ERROR: Module signing did not produce a valid signature" >&2
    return 1
  }
}

install_module() {
  [[ $EUID -eq 0 ]] || {
    echo "ERROR: install must be run with sudo" >&2
    exit 1
  }

  local source_module="$BUILD_DIR/$MODULE_RELATIVE"
  local target="/lib/modules/$KERNEL_RELEASE/$INSTALL_RELATIVE"

  [[ -f "$source_module" ]] || {
    echo "ERROR: Build the module as the regular user before installing:" >&2
    echo "  ./tools/build-audio-quirk.sh $KERNEL_SOURCE" >&2
    exit 1
  }
  validate_module "$source_module"

  # modinfo requires the staged file to retain a recognised module suffix.
  local staged_target="${target%.ko}.new.ko"

  install -D -m 0644 "$source_module" "$staged_target"
  if ! sign_for_secure_boot "$staged_target"; then
    rm -f "$staged_target"
    exit 1
  fi
  mv -f "$staged_target" "$target"
  depmod "$KERNEL_RELEASE"
  update-initramfs -u -k "$KERNEL_RELEASE"
  echo "Installed: $target"
  echo "Signer: $(modinfo -F signer "$target")"
  echo "Reboot, then verify with: aplay -l"
}

remove_module() {
  [[ $EUID -eq 0 ]] || {
    echo "ERROR: remove must be run with sudo" >&2
    exit 1
  }

  local target="/lib/modules/$KERNEL_RELEASE/$INSTALL_RELATIVE"
  rm -f "$target"
  depmod "$KERNEL_RELEASE"
  update-initramfs -u -k "$KERNEL_RELEASE"
  echo "Removed override: $target"
}

case "${1:-}" in
  -h|--help)
    usage
    ;;
  install)
    KERNEL_SOURCE="${2:-}"
    BUILD_DIR="${BUILD_DIR:-$KERNEL_SOURCE}"
    [[ -n "$KERNEL_SOURCE" ]] || {
      usage >&2
      exit 2
    }
    install_module
    ;;
  remove)
    remove_module
    ;;
  "")
    usage >&2
    exit 2
    ;;
  *)
    BUILD_DIR="${BUILD_DIR:-$KERNEL_SOURCE}"
    build_module
    ;;
esac
