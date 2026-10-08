#!/bin/sh
# Installs Lume on Linux from the latest GitHub release.
#
#   curl -fsSL https://raw.githubusercontent.com/Lume-agents/Lume/main/scripts/install.sh | sh
#
# Pass options after `sh -s --`:
#   curl -fsSL .../install.sh | sh -s -- --version 0.15.4 --method appimage
#
# Options:
#   --version X.Y.Z   Install that release instead of the latest one.
#   --method M        auto (default), deb, rpm or appimage.
#   --dry-run         Print what would happen without downloading or installing.
#   -h, --help        Show this help.
#
# Environment: LUME_INSTALL_DIR sets where the AppImage goes (default ~/.local/bin).
#
# The script only downloads assets published on https://github.com/Lume-agents/Lume/releases.
# It uses sudo for the .deb and .rpm packages and never for the AppImage.
set -eu

REPO="Lume-agents/Lume"
VERSION=""
METHOD="auto"
DRY_RUN=0

say() { printf '%s\n' "$*"; }
fail() { printf 'lume-install: %s\n' "$*" >&2; exit 1; }
usage() { sed -n '2,19p' "$0" 2>/dev/null | sed 's/^# \{0,1\}//' || true; }

while [ $# -gt 0 ]; do
  case "$1" in
    --version) [ $# -ge 2 ] || fail "--version needs a value"; VERSION="${2#v}"; shift 2 ;;
    --version=*) VERSION="${1#--version=}"; VERSION="${VERSION#v}"; shift ;;
    --method) [ $# -ge 2 ] || fail "--method needs a value"; METHOD="$2"; shift 2 ;;
    --method=*) METHOD="${1#--method=}"; shift ;;
    --dry-run) DRY_RUN=1; shift ;;
    -h|--help) usage; exit 0 ;;
    *) fail "unknown option: $1 (try --help)" ;;
  esac
done

case "$METHOD" in auto|deb|rpm|appimage) ;; *) fail "--method must be auto, deb, rpm or appimage" ;; esac

OS="$(uname -s)"
ARCH="$(uname -m)"
case "$OS" in
  Linux) ;;
  Darwin) fail "macOS builds are still being validated. Download the .dmg from https://github.com/$REPO/releases/latest when you want to try it." ;;
  *) fail "unsupported system: $OS. On Windows use install.ps1." ;;
esac
case "$ARCH" in
  x86_64|amd64) ;;
  *) fail "Lume publishes Linux packages for x86_64 only (this machine is $ARCH)." ;;
esac

command -v curl >/dev/null 2>&1 || fail "curl is required"

if [ -n "$VERSION" ]; then
  API="https://api.github.com/repos/$REPO/releases/tags/v$VERSION"
else
  API="https://api.github.com/repos/$REPO/releases/latest"
fi
RELEASE="$(curl -fsSL -H 'Accept: application/vnd.github+json' "$API")" || fail "could not read the release from $API"
TAG="$(printf '%s' "$RELEASE" | grep -o '"tag_name": *"[^"]*"' | head -n 1 | sed 's/.*"\([^"]*\)"$/\1/')"
[ -n "$TAG" ] || fail "no release found"
URLS="$(printf '%s' "$RELEASE" | grep -o '"browser_download_url": *"[^"]*"' | sed 's/.*"\(http[^"]*\)"$/\1/')"

asset() { printf '%s\n' "$URLS" | grep -E "$1" | head -n 1; }

if [ "$METHOD" = "auto" ]; then
  if command -v apt-get >/dev/null 2>&1 && command -v dpkg >/dev/null 2>&1; then METHOD="deb"
  elif command -v dnf >/dev/null 2>&1 || command -v zypper >/dev/null 2>&1 || command -v yum >/dev/null 2>&1; then METHOD="rpm"
  else METHOD="appimage"; fi
fi

case "$METHOD" in
  deb) PATTERN='_amd64\.deb$' ;;
  rpm) PATTERN='\.x86_64\.rpm$' ;;
  appimage) PATTERN='_amd64\.AppImage$' ;;
esac
URL="$(asset "$PATTERN")"
[ -n "$URL" ] || fail "release $TAG has no $METHOD package"
FILE="${URL##*/}"

say "Lume $TAG · $METHOD · $FILE"
if [ "$DRY_RUN" = 1 ]; then
  say "[dry run] would download $URL"
  case "$METHOD" in
    deb) say "[dry run] would run: sudo apt-get install -y ./$FILE" ;;
    rpm) say "[dry run] would run: sudo dnf install -y ./$FILE (or zypper/yum)" ;;
    appimage) say "[dry run] would install to ${LUME_INSTALL_DIR:-$HOME/.local/bin}/lume and add a launcher entry" ;;
  esac
  exit 0
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT INT TERM
say "Downloading…"
curl -fL --progress-bar -o "$TMP/$FILE" "$URL" || fail "download failed"
if command -v sha256sum >/dev/null 2>&1; then say "SHA-256: $(sha256sum "$TMP/$FILE" | cut -d ' ' -f 1)"; fi

as_root() {
  if [ "$(id -u)" = 0 ]; then "$@"
  elif command -v sudo >/dev/null 2>&1; then sudo "$@"
  else fail "root privileges are needed to install the package (sudo not found)"; fi
}

case "$METHOD" in
  deb) as_root apt-get install -y "$TMP/$FILE" ;;
  rpm)
    if command -v dnf >/dev/null 2>&1; then as_root dnf install -y "$TMP/$FILE"
    elif command -v zypper >/dev/null 2>&1; then as_root zypper --non-interactive install --allow-unsigned-rpm "$TMP/$FILE"
    else as_root yum install -y "$TMP/$FILE"; fi ;;
  appimage)
    DIR="${LUME_INSTALL_DIR:-$HOME/.local/bin}"
    mkdir -p "$DIR"
    cp "$TMP/$FILE" "$DIR/lume"
    chmod +x "$DIR/lume"
    APPS="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
    mkdir -p "$APPS"
    printf '[Desktop Entry]\nType=Application\nName=Lume\nComment=A local monitor for your AI agents\nExec=%s\nTerminal=false\nCategories=Development;\n' "$DIR/lume" > "$APPS/lume.desktop"
    say "Installed $DIR/lume"
    case ":$PATH:" in *":$DIR:"*) ;; *) say "Add $DIR to your PATH to start it with: lume" ;; esac ;;
esac
say "Lume $TAG is installed. Open it from your applications menu."
