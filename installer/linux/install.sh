#!/bin/sh
# Install Phone Remote for the current user (no root needed). Run from the unpacked folder:
#   ./install.sh            install into ~/.local
#   ./install.sh --remove   remove it again
set -eu

HERE=$(cd "$(dirname "$0")" && pwd)
PREFIX="${PREFIX:-$HOME/.local}"
BIN="$PREFIX/bin"
APPS="$PREFIX/share/applications"
ICONS="$PREFIX/share/icons/hicolor/512x512/apps"

if [ "${1:-}" = "--remove" ]; then
    "$BIN/phone-remote" autostart off >/dev/null 2>&1 || true
    rm -f "$BIN/phone-remote" "$APPS/phone-remote.desktop" "$ICONS/phone-remote.png"
    echo "Phone Remote removed. Your pairings and settings are kept in ~/.config/phone-remote (delete that folder to forget them)."
    exit 0
fi

mkdir -p "$BIN" "$APPS" "$ICONS"
install -m 755 "$HERE/phone-remote" "$BIN/phone-remote"
install -m 644 "$HERE/phone-remote.png" "$ICONS/phone-remote.png"
# The launcher must point at the installed binary, even when ~/.local/bin is not on PATH.
sed "s|^Exec=phone-remote |Exec=$BIN/phone-remote |" "$HERE/phone-remote.desktop" > "$APPS/phone-remote.desktop"
chmod 644 "$APPS/phone-remote.desktop"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$APPS" 2>/dev/null || true

echo "Installed to $BIN/phone-remote"
echo "Open it from your applications menu, or run:  $BIN/phone-remote open"
echo "Start at login:                                $BIN/phone-remote autostart on"
