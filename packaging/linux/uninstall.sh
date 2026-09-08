#!/usr/bin/env bash
set -euo pipefail

if [ "$(id -u)" -eq 0 ]; then
  echo "Do not run this uninstaller with sudo/root. It removes Manager for the current user." >&2
  exit 1
fi

DATA_HOME="${XDG_DATA_HOME:-$HOME/.local/share}"
CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}"
INSTALL_DIR="$DATA_HOME/manager/bin"
BIN_DIR="$HOME/.local/bin"
APP_DIR="$DATA_HOME/applications"
AUTOSTART_DIR="$CONFIG_HOME/autostart"

pkill -x Manager 2>/dev/null || true
pkill -x ManagerUI 2>/dev/null || true
pkill -x ManagerCapture 2>/dev/null || true

rm -f "$BIN_DIR/Manager" "$BIN_DIR/ManagerUI" "$BIN_DIR/ManagerCapture"
rm -f "$APP_DIR/manager.desktop" "$APP_DIR/manager-capture.desktop"
rm -f "$AUTOSTART_DIR/Manager.desktop"
rm -rf "$INSTALL_DIR"

if command -v kbuildsycoca6 >/dev/null 2>&1; then
  kbuildsycoca6 >/dev/null 2>&1 || true
elif command -v kbuildsycoca5 >/dev/null 2>&1; then
  kbuildsycoca5 >/dev/null 2>&1 || true
fi

printf 'Manager binaries and autostart entry were removed.\n'
printf 'User config/logs were preserved at: %s\n' "$DATA_HOME/manager"
