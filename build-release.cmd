@echo off
setlocal

echo [Tack] Stopping running Tack processes that may lock release binaries...
for %%P in (tack-agent.exe tack-ui.exe tack-capture.exe) do (
    taskkill /F /IM %%P >nul 2>&1
)

echo [Tack] Building release binaries...
cargo build --release
set EXIT_CODE=%ERRORLEVEL%

if not "%EXIT_CODE%"=="0" (
    echo [Tack] Build failed with exit code %EXIT_CODE%.
    exit /b %EXIT_CODE%
)

echo [Tack] Build complete:
echo   target\release\tack-agent.exe
echo   target\release\tack-ui.exe
echo   target\release\tack-capture.exe
exit /b 0
