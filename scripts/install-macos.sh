#!/bin/sh
# Installs or updates Lume on macOS (Apple Silicon, macOS 15+).
#
#   curl -fsSL https://raw.githubusercontent.com/Lume-agents/Lume/main/scripts/install-macos.sh | sh
#
# Provisional path until the macOS build is signed with Developer ID and
# notarized (#13). Files downloaded by curl carry no quarantine attribute, so
# Gatekeeper does not block the first launch. This script never removes
# quarantine or changes Gatekeeper settings.
#
# Environment:
#   LUME_INSTALL_DIR  directory that receives Lume.app (default: /Applications,
#                     or ~/Applications when /Applications is not writable)
#   LUME_VERSION      version to install, such as 0.15.4 (default: latest)
#
# Everything runs inside main so a partially downloaded script does nothing.

set -eu

main() {
  repo="Lume-agents/Lume"
  app_name="Lume.app"
  min_macos_major=15

  say() { printf 'lume: %s\n' "$*"; }
  fail() { printf 'lume: error: %s\n' "$*" >&2; exit 1; }
  fetch() { curl --proto '=https' --tlsv1.2 --fail --location --silent --show-error "$@"; }

  [ "$(uname -s)" = "Darwin" ] || fail "this installer is for macOS. Other platforms: https://github.com/$repo/releases/latest"

  # uname -m reports x86_64 under Rosetta, so ask the hardware instead.
  [ "$(sysctl -in hw.optional.arm64 2>/dev/null || true)" = "1" ] \
    || fail "Lume for macOS currently requires Apple Silicon (arm64). Intel Macs are not supported yet."

  macos_version="$(sw_vers -productVersion)"
  macos_major="${macos_version%%.*}"
  [ "$macos_major" -ge "$min_macos_major" ] 2>/dev/null \
    || fail "Lume requires macOS $min_macos_major or later (found $macos_version)."

  version="${LUME_VERSION:-}"
  tmp="$(mktemp -d "${TMPDIR:-/tmp}/lume-install.XXXXXX")"
  trap 'rm -rf "$tmp"' EXIT INT TERM

  if [ -z "$version" ]; then
    say "checking the latest release"
    fetch --output "$tmp/latest.json" "https://github.com/$repo/releases/latest/download/latest.json" \
      || fail "could not download the release manifest."
    version="$(plutil -extract version raw -o - "$tmp/latest.json" 2>/dev/null)" \
      || fail "the release manifest has no version."
  fi
  version="${version#v}"
  case "$version" in
    [0-9]*.[0-9]*.[0-9]*) ;;
    *) fail "unexpected version '$version'." ;;
  esac

  archive="Lume_${version}_aarch64.app.tar.gz"
  say "downloading Lume $version"
  fetch --output "$tmp/$archive" "https://github.com/$repo/releases/download/v$version/$archive" \
    || fail "could not download $archive."

  mkdir "$tmp/extract"
  tar -xzf "$tmp/$archive" -C "$tmp/extract" || fail "could not extract $archive."
  new_app="$tmp/extract/$app_name"
  [ -d "$new_app/Contents/MacOS" ] || fail "$archive does not contain $app_name."
  extracted_version="$(plutil -extract CFBundleShortVersionString raw -o - "$new_app/Contents/Info.plist" 2>/dev/null || true)"
  [ "$extracted_version" = "$version" ] \
    || fail "$archive contains version '${extracted_version:-unknown}', expected $version."

  if [ -n "${LUME_INSTALL_DIR:-}" ]; then
    install_dir="$LUME_INSTALL_DIR"
  elif [ -w /Applications ]; then
    install_dir="/Applications"
  else
    install_dir="$HOME/Applications"
  fi
  mkdir -p "$install_dir" || fail "could not create $install_dir."
  [ -w "$install_dir" ] || fail "$install_dir is not writable."
  target="$install_dir/$app_name"

  was_running=0
  if [ -d "$target" ]; then
    binary_dir="$target/Contents/MacOS/"
    if pgrep -f "$binary_dir" >/dev/null 2>&1; then
      was_running=1
      say "closing the running Lume"
      pkill -TERM -f "$binary_dir" || true
      waited=0
      while pgrep -f "$binary_dir" >/dev/null 2>&1; do
        [ "$waited" -lt 10 ] || fail "Lume is still running. Quit it from the menu bar and run the installer again."
        sleep 1
        waited=$((waited + 1))
      done
    fi
  fi

  # Stage next to the target so the swap is a rename on the same volume and an
  # interrupted install never leaves a half-copied Lume.app in place.
  staged="$install_dir/.$app_name.new.$$"
  previous="$install_dir/.$app_name.old.$$"
  rm -rf "$staged" "$previous"
  ditto "$new_app" "$staged" || { rm -rf "$staged"; fail "could not copy Lume into $install_dir."; }
  if [ -e "$target" ]; then
    mv "$target" "$previous" || { rm -rf "$staged"; fail "could not replace $target."; }
  fi
  if ! mv "$staged" "$target"; then
    [ -e "$previous" ] && mv "$previous" "$target"
    rm -rf "$staged"
    fail "could not move Lume into $target."
  fi
  rm -rf "$previous"

  say "installed Lume $version at $target"
  say "this build is not notarized yet; updates arrive through Settings -> About."

  if [ "$was_running" -eq 1 ]; then
    open "$target" || true
  else
    say "open it from $install_dir or Spotlight."
  fi
}

main "$@"
