#!/usr/bin/env bash
set -euo pipefail

PROJECT_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
SOURCE_ROOT="$PROJECT_ROOT/audio/ucm2"
UCM_ROOT="${ZENBOOK_DUO_UCM_ROOT:-/usr/share/alsa/ucm2}"
STATE_ROOT="${ZENBOOK_DUO_UCM_STATE_ROOT:-/var/lib/zenbook-duo/audio-ucm-backup}"
PATCH_FILE="$SOURCE_ROOT/ux8407aa-alsa-ucm.patch"
MARKER="$STATE_ROOT/installed"

usage() {
  cat <<'EOF'
Usage:
  sudo ./tools/configure-audio-ucm.sh install
  ./tools/configure-audio-ucm.sh activate
  ./tools/configure-audio-ucm.sh validate
  ./tools/configure-audio-ucm.sh status
  sudo ./tools/configure-audio-ucm.sh remove
EOF
}

require_root() {
  if [[ $EUID -ne 0 ]]; then
    echo "ERROR: This action must run as root" >&2
    exit 1
  fi
}

require_files() {
  local path
  for path in \
    "$UCM_ROOT/codecs/cs42l43/init.conf" \
    "$UCM_ROOT/sof-soundwire/sof-soundwire.conf" \
    "$UCM_ROOT/sof-soundwire/cs42l43-spk.conf"; do
    if [[ ! -e "$path" ]]; then
      echo "ERROR: Missing required UCM file: $path" >&2
      exit 1
    fi
  done
}

apply_overlay() {
  local root="$1"

  patch --batch --forward --silent -d "$root" -p1 < "$PATCH_FILE"
  install -D -m 0644 \
    "$SOURCE_ROOT/codecs/cs42l43-spk/init.conf" \
    "$root/codecs/cs42l43-spk/init.conf"
  install -D -m 0644 \
    "$SOURCE_ROOT/codecs/cs42l43-spk+cs35l56/init.conf" \
    "$root/codecs/cs42l43-spk+cs35l56/init.conf"
  ln -sfn cs42l43-spk+cs35l56 "$root/codecs/cs35l56+cs42l43-spk"
  ln -sfn cs42l43-spk.conf \
    "$root/sof-soundwire/cs35l56+cs42l43-spk.conf"
  ln -sfn cs42l43-spk.conf \
    "$root/sof-soundwire/cs42l43-spk+cs35l56.conf"
}

validate_tree() {
  local root="$1"
  local output

  output="$(ALSA_CONFIG_UCM2="$root" alsaucm -c hw:0 dump text 2>&1)" || {
    printf '%s\n' "$output" >&2
    return 1
  }
  if ! grep -q 'PlaybackPCM.*hw:.*,[[:space:]]*2' <<<"$output"; then
    echo "ERROR: UCM parses, but the speaker PCM (device 2) was not found" >&2
    return 1
  fi
}

validate() {
  local temp_root
  require_files

  if [[ -e "$MARKER" ]]; then
    validate_tree "$UCM_ROOT"
    echo "Installed UCM validation passed: Speaker routes to ALSA device 2"
    return
  fi

  temp_root="$(mktemp -d)"
  trap 'rm -rf -- "$temp_root"' RETURN
  cp -a "$UCM_ROOT/." "$temp_root/"
  apply_overlay "$temp_root"
  validate_tree "$temp_root"
  echo "UCM validation passed: Speaker routes to ALSA device 2"
}

install_ucm() {
  local temp_root
  require_root
  require_files

  if [[ -e "$MARKER" ]]; then
    echo "UX8407AA audio UCM fix is already installed"
    exit 0
  fi

  temp_root="$(mktemp -d)"
  trap 'rm -rf -- "$temp_root"' RETURN
  cp -a "$UCM_ROOT/." "$temp_root/"
  apply_overlay "$temp_root"
  validate_tree "$temp_root"

  mkdir -p "$STATE_ROOT/codecs/cs42l43" "$STATE_ROOT/sof-soundwire"
  cp -a "$UCM_ROOT/codecs/cs42l43/init.conf" \
    "$STATE_ROOT/codecs/cs42l43/init.conf"
  cp -a "$UCM_ROOT/sof-soundwire/sof-soundwire.conf" \
    "$STATE_ROOT/sof-soundwire/sof-soundwire.conf"

  if ! apply_overlay "$UCM_ROOT"; then
    cp -a "$STATE_ROOT/codecs/cs42l43/init.conf" \
      "$UCM_ROOT/codecs/cs42l43/init.conf"
    cp -a "$STATE_ROOT/sof-soundwire/sof-soundwire.conf" \
      "$UCM_ROOT/sof-soundwire/sof-soundwire.conf"
    echo "ERROR: UCM installation failed; original files restored" >&2
    exit 1
  fi
  date --iso-8601=seconds > "$MARKER"
  echo "Installed UX8407AA CS42L43 + CS35L56 UCM routing fix"
  echo "Run as the desktop user: ./tools/configure-audio-ucm.sh activate"
}

activate() {
  local volume_state

  if [[ $EUID -eq 0 ]]; then
    echo "ERROR: Run activate as the logged-in desktop user, not with sudo" >&2
    exit 1
  fi
  if [[ ! -e "$MARKER" ]]; then
    echo "ERROR: Install the UCM fix first" >&2
    exit 1
  fi

  systemctl --user restart wireplumber pipewire pipewire-pulse
  sleep 2

  # Force a route state edge so stale pre-fix mixer state is synchronized.
  volume_state="$(wpctl get-volume @DEFAULT_AUDIO_SINK@)"
  wpctl set-mute @DEFAULT_AUDIO_SINK@ 1
  wpctl set-mute @DEFAULT_AUDIO_SINK@ 0
  if grep -q '\[MUTED\]' <<<"$volume_state"; then
    wpctl set-mute @DEFAULT_AUDIO_SINK@ 1
  fi

  if ! wpctl inspect @DEFAULT_AUDIO_SINK@ |
      grep -q 'device.profile.description = "Speaker"'; then
    echo "ERROR: Default audio sink is not the internal Speaker" >&2
    exit 1
  fi
  echo "Audio services restarted; internal Speaker route is active"
}

remove_ucm() {
  require_root
  if [[ ! -e "$MARKER" ]]; then
    echo "UX8407AA audio UCM fix is not installed"
    exit 0
  fi

  cp -a "$STATE_ROOT/codecs/cs42l43/init.conf" \
    "$UCM_ROOT/codecs/cs42l43/init.conf"
  cp -a "$STATE_ROOT/sof-soundwire/sof-soundwire.conf" \
    "$UCM_ROOT/sof-soundwire/sof-soundwire.conf"
  rm -rf -- "$UCM_ROOT/codecs/cs42l43-spk"
  rm -rf -- "$UCM_ROOT/codecs/cs42l43-spk+cs35l56"
  rm -f -- "$UCM_ROOT/codecs/cs35l56+cs42l43-spk"
  rm -f -- "$UCM_ROOT/sof-soundwire/cs35l56+cs42l43-spk.conf"
  rm -f -- "$UCM_ROOT/sof-soundwire/cs42l43-spk+cs35l56.conf"
  rm -rf -- "$STATE_ROOT"
  echo "Removed UX8407AA audio UCM fix and restored distribution files"
}

status() {
  if [[ -e "$MARKER" ]]; then
    echo "installed ($(cat "$MARKER"))"
  else
    echo "not installed"
  fi
}

case "${1:-}" in
  install) install_ucm ;;
  activate) activate ;;
  validate) validate ;;
  status) status ;;
  remove) remove_ucm ;;
  *) usage; exit 2 ;;
esac
