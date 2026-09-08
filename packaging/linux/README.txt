Manager for Linux
=================

Install
-------
1. Extract this archive.
2. Open a terminal in the extracted folder.
3. Run: ./install.sh
4. Manager will be installed for the current user and launched when a graphical session is available.
5. Add/pair your Zalo bot in Manager. Once configured, Manager registers background autostart automatically.

Do not run install.sh with sudo.

Installed files
---------------
Binaries: ~/.local/share/manager/bin/
Command symlinks: ~/.local/bin/
Desktop launcher: ~/.local/share/applications/manager.desktop
Autostart after configuration: ~/.config/autostart/Manager.desktop
Config/logs: ~/.local/share/manager/

Uninstall
---------
Run: ./uninstall.sh

The uninstaller removes binaries, launchers, and autostart but preserves config/logs.
