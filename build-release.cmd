@echo off
setlocal

if "%VERSION%"=="" set VERSION=3.0.3

echo [Manager] Stopping running processes...
for %%P in (Manager.exe ManagerUI.exe ManagerCapture.exe) do (
    taskkill /F /IM %%P >nul 2>&1
)

if exist dist rmdir /S /Q dist
mkdir dist

echo [Manager] Formatting...
go fmt ./...
if errorlevel 1 exit /b %errorlevel%

echo [Manager] Vetting...
go vet ./...
if errorlevel 1 exit /b %errorlevel%

echo [Manager] Testing...
go test ./...
if errorlevel 1 exit /b %errorlevel%

set CGO_ENABLED=0
set LDFLAGS=-s -w -H=windowsgui -X github.com/Dyu-36/screenshot/internal/appinfo.Version=%VERSION%

echo [Manager] Building Manager.exe version %VERSION%...
go build -trimpath -ldflags="%LDFLAGS%" -o dist\Manager.exe ./cmd/manager
if errorlevel 1 exit /b %errorlevel%

echo [Manager] Building ManagerUI.exe...
go build -trimpath -ldflags="%LDFLAGS%" -o dist\ManagerUI.exe ./cmd/manager-ui
if errorlevel 1 exit /b %errorlevel%

echo [Manager] Building ManagerCapture.exe...
go build -trimpath -ldflags="%LDFLAGS%" -o dist\ManagerCapture.exe ./cmd/manager-capture
if errorlevel 1 exit /b %errorlevel%

echo [Manager] Build complete:
for %%F in (Manager.exe ManagerUI.exe ManagerCapture.exe) do (
    if not exist dist\%%F (
        echo Missing dist\%%F
        exit /b 1
    )
    echo   dist\%%F
)
exit /b 0
