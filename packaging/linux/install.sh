#!/usr/bin/env bash
set -euo pipefail

if [ "$(id -u)" -eq 0 ]; then
  echo "Do not run this installer with sudo/root. It installs Manager for the current user." >&2
  exit 1
fi

HERE="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"
INSTALL_DIR="$DATA_HOME/manager/bin"
BIN_DIR="$HOME/.local/bin"
APP_DIR="$DATA_HOME/applications"
AUTOSTART_DIR="$CONFIG_HOME/autostart"

for file in Manager ManagerUI ManagerCapture; do
  if [ ! -f "$HERE/$file" ]; then
    echo "Missing $file next to install.sh" >&2
    exit 1
  fi
done

pkill -x Manager 2>/dev/null || true
pkill -x ManagerUI 2>/dev/null || true
pkill -x ManagerCapture 2>/dev/null || true

mkdir -p "$INSTALL_DIR" "$BIN_DIR" "$APP_DIR"
install -m 0755 "$HERE/Manager" "$INSTALL_DIR/Manager"
install -m 0755 "$HERE/ManagerUI" "$INSTALL_DIR/ManagerUI"
install -m 0755 "$HERE/ManagerCapture" "$INSTALL_DIR/ManagerCapture"
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

printf '\nManager installed successfully.\n'
printf 'Install directory: %s\n' "$INSTALL_DIR"
printf 'Config/data are stored under: %s\n' "$DATA_HOME/manager"
printf 'Start Manager from your application launcher or run: %s\n' "$INSTALL_DIR/Manager"
printf 'After a bot is configured, Manager registers its background autostart automatically.\n'

if [ -n "${WAYLAND_DISPLAY:-}${DISPLAY:-}" ]; then
  nohup "$INSTALL_DIR/Manager" >/dev/null 2>&1 &
fi
