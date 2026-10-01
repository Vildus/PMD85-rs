#!/bin/sh
# User-level desktop integration for the PMD 85 emulator.
#
# Installs the application entry and the icon into the user's data
# dirs (~/.local/share), so Wayland/X11 taskbars, dashes and app
# launchers can find the app and its icon. No root, nothing outside
# the user profile:
#
#   ~/.local/share/applications/pmd85.desktop
#   ~/.local/share/icons/hicolor/128x128/apps/pmd85.png
#       (or ~/.local/share/pixmaps/pmd85.png when no ImageMagick)
#
# Run again any time after moving the repo or rebuilding.

set -e

# The repo root (this script lives in dist/).
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/release/pmd85"
DESKTOP_SRC="$ROOT/dist/pmd85.desktop"
ICON_SRC="$ROOT/assets/icon.png"

[ -x "$BIN" ] || {
    echo "error: $BIN does not exist (or is not executable)."
    echo "build it first:  cargo build --release"
    exit 1
}

DATA="${XDG_DATA_HOME:-$HOME/.local/share}"
APPS="$DATA/applications"
mkdir -p "$APPS"

# The icon: 128x128, nearest-neighbor so the pixel art stays crisp.
# Without ImageMagick, fall back to the original PNG in pixmaps
# (taskbars scale it themselves).
ICONS="$DATA/icons/hicolor/128x128/apps"
if command -v magick >/dev/null 2>&1; then
    mkdir -p "$ICONS"
    magick "$ICON_SRC" -filter point -resize 128x128 "$ICONS/pmd85.png"
elif command -v convert >/dev/null 2>&1; then
    mkdir -p "$ICONS"
    convert "$ICON_SRC" -filter point -resize 128x128 "$ICONS/pmd85.png"
else
    mkdir -p "$DATA/pixmaps"
    cp "$ICON_SRC" "$DATA/pixmaps/pmd85.png"
fi

# The application entry, with the real binary path in Exec.
sed "s|@BIN@|$BIN|g" "$DESKTOP_SRC" > "$APPS/pmd85.desktop"

# Refresh the caches where they exist.
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$APPS" || true
command -v kbuildsycoca6 >/dev/null 2>&1 && kbuildsycoca6 --noincremental >/dev/null 2>&1 || true

echo "installed:"
echo "  $APPS/pmd85.desktop"
echo "  icon + caches refreshed"
echo
echo "If the taskbar does not pick the icon up immediately, relog or"
echo "restart the panel/dock. (Wayland needs the window to set the"
echo "app_id, which the app does; GNOME shows no titlebar icons.)"
