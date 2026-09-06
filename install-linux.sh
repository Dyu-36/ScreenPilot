#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"
./build-release.sh

DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
INSTALL_DIR="$DATA_HOME/manager/bin"
BIN_DIR="$HOME/.local/bin"
APP_DIR="$DATA_HOME/applications"
mkdir -p "$INSTALL_DIR" "$BIN_DIR" "$APP_DIR"
install -m 0755 dist/Manager "$INSTALL_DIR/Manager"
install -m 0755 dist/ManagerUI "$INSTALL_DIR/ManagerUI"
install -m 0755 dist/ManagerCapture "$INSTALL_DIR/ManagerCapture"
ln -sfn "$INSTALL_DIR/Manager" "$BIN_DIR/Manager"
ln -sfn "$INSTALL_DIR/ManagerUI" "$BIN_DIR/ManagerUI"
ln -sfn "$INSTALL_DIR/ManagerCapture" "$BIN_DIR/ManagerCapture"

cat > "$APP_DIR/manager.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Manager
Comment=Lightweight Zalo-controlled manager
Exec="$INSTALL_DIR/Manager"
Terminal=false
Categories=Utility;
DESKTOP

cat > "$APP_DIR/manager-capture.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Manager Capture
NoDisplay=true
Exec="$INSTALL_DIR/ManagerCapture"
Terminal=false
X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2
DESKTOP
chmod 0600 "$APP_DIR/manager.desktop" "$APP_DIR/manager-capture.desktop"

if command -v kbuildsycoca6 >/dev/null 2>&1; then
  kbuildsycoca6 >/dev/null 2>&1 || true
elif command -v kbuildsycoca5 >/dev/null 2>&1; then
  kbuildsycoca5 >/dev/null 2>&1 || true
fi

printf '\nInstalled to: %s\n' "$INSTALL_DIR"
printf 'Launch with: %s\n' "$INSTALL_DIR/Manager"
printf 'If ~/.local/bin is in PATH, you can simply run: Manager\n'
