@echo off
setlocal

echo [Manager] Stopping running Manager processes that may lock release binaries...
for %%P in (Manager.exe ManagerUI.exe ManagerCapture.exe) do (
    taskkill /F /IM %%P >nul 2>&1
)

echo [Manager] Building release binaries...
cargo build --release
set EXIT_CODE=%ERRORLEVEL%

if not "%EXIT_CODE%"=="0" (
    echo [Manager] Build failed with exit code %EXIT_CODE%.
    exit /b %EXIT_CODE%
)

echo [Manager] Build complete:
echo   target\release\Manager.exe
echo   target\release\ManagerUI.exe
echo   target\release\ManagerCapture.exe
exit /b 0
