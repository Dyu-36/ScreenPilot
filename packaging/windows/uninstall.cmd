@echo off
setlocal EnableExtensions

set "INSTALL_DIR=%LOCALAPPDATA%\Manager\bin"
set "SHORTCUT=%APPDATA%\Microsoft\Windows\Start Menu\Programs\Manager.lnk"

for %%P in (Manager.exe ManagerUI.exe ManagerCapture.exe) do (
    taskkill /F /IM %%P >nul 2>&1
)

reg delete "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v Manager /f >nul 2>&1
reg delete "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v TackAgent /f >nul 2>&1

if exist "%SHORTCUT%" del /F /Q "%SHORTCUT%"
if exist "%INSTALL_DIR%" rmdir /S /Q "%INSTALL_DIR%"

echo Manager binaries, Start Menu shortcut, and autostart entry were removed.
echo User config/logs were preserved at: %APPDATA%\manager
pause
exit /b 0
