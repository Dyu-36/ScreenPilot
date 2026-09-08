Manager for Windows
===================

Install
-------
1. Extract this ZIP to a normal folder.
2. Double-click install.cmd.
3. Manager is copied to %LOCALAPPDATA%\Manager\bin and a Start Menu shortcut is created.
4. Manager opens automatically after installation.
5. Add/pair your Zalo bot. Once configured, Manager registers itself in the current user's Windows Run key for background autostart.

No administrator rights and no Go installation are required.

Pairing and screenshots
-----------------------
1. Use a separate Zalo bot token for each running computer. Two computers polling
   the same bot can consume each other's messages, including /pair and /anh.
2. For a saved bot, click "Tao ma /pair". For a new bot, paste its complete token
   and click "Luu & ghep cap". Send the displayed /pair command to that exact bot
   within 60 seconds. Creating another code invalidates the previous attempt.
3. Wait for the pairing confirmation, then send /anh to receive the primary
   Windows desktop. Manager switches to background operation after pairing.
4. The Windows user session must be logged in with an unlocked desktop.

This Windows fix prevents cancelled pairing attempts from clearing a newer
code, serializes pairing against background polling, validates the bot token,
and displays permanent API errors instead of silently retrying them.
Windows capture keeps GDI operations on the same OS thread.
Existing bot/device configuration is preserved when installing over a release.

If pairing fails, check the status text and %APPDATA%\manager\manager.log.
HTTP 404 on getMe means the supplied token was rejected; copy the complete
token for the correct bot. A successful getMe alone does not confirm pairing.

Installed files
---------------
Binaries: %LOCALAPPDATA%\Manager\bin\
Start Menu shortcut: Manager
Autostart after configuration: HKCU\Software\Microsoft\Windows\CurrentVersion\Run\Manager
Config/logs: %APPDATA%\manager\

Uninstall
---------
Double-click uninstall.cmd.

The uninstaller removes binaries, the Start Menu shortcut, and autostart registration but preserves config/logs.
