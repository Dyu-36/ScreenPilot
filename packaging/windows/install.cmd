@echo off
setlocal EnableExtensions

set "HERE=%~dp0"
set "INSTALL_DIR=%LOCALAPPDATA%\Manager\bin"
set "START_MENU=%APPDATA%\Microsoft\Windows\Start Menu\Programs"

for %%F in (Manager.exe ManagerUI.exe ManagerCapture.exe) do (
    if not exist "%HERE%%%F" (
        echo Missing %%F next to install.cmd
        pause
        exit /b 1
    )
)

for %%P in (Manager.exe ManagerUI.exe ManagerCapture.exe) do (
    taskkill /F /IM %%P >nul 2>&1
)

if not exist "%INSTALL_DIR%" mkdir "%INSTALL_DIR%"
copy /Y "%HERE%Manager.exe" "%INSTALL_DIR%\Manager.exe" >nul
copy /Y "%HERE%ManagerUI.exe" "%INSTALL_DIR%\ManagerUI.exe" >nul
copy /Y "%HERE%ManagerCapture.exe" "%INSTALL_DIR%\ManagerCapture.exe" >nul

powershell -NoProfile -ExecutionPolicy Bypass -Command "$w=New-Object -ComObject WScript.Shell; $s=$w.CreateShortcut([IO.Path]::Combine($env:APPDATA,'Microsoft\Windows\Start Menu\Programs\Manager.lnk')); $s.TargetPath=[IO.Path]::Combine($env:LOCALAPPDATA,'Manager\bin\Manager.exe'); $s.WorkingDirectory=[IO.Path]::Combine($env:LOCALAPPDATA,'Manager\bin'); $s.Save()"
if errorlevel 1 (
    echo Warning: could not create Start Menu shortcut.
)

echo.
echo Manager installed successfully.
echo Install directory: %INSTALL_DIR%
echo Config/logs: %APPDATA%\manager
echo After a bot is configured, Manager registers background autostart automatically.
echo.
start "" "%INSTALL_DIR%\Manager.exe"
exit /b 0
