#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")"

VERSION="${VERSION:-3.0.4}"

echo "[Manager] Formatting..."
gofmt -w ./cmd ./internal

echo "[Manager] Vetting..."
go vet ./...

echo "[Manager] Testing..."
go test ./...

rm -rf dist
mkdir -p dist

export CGO_ENABLED=0
LDFLAGS="-s -w -X github.com/Dyu-36/screenshot/internal/appinfo.Version=${VERSION}"

echo "[Manager] Building Linux binaries version ${VERSION}..."
go build -trimpath -ldflags "$LDFLAGS" -o dist/Manager ./cmd/manager
go build -trimpath -ldflags "$LDFLAGS" -o dist/ManagerUI ./cmd/manager-ui
go build -trimpath -ldflags "$LDFLAGS" -o dist/ManagerCapture ./cmd/manager-capture
chmod +x dist/Manager dist/ManagerUI dist/ManagerCapture

echo "[Manager] Build complete:"
ls -lh dist/Manager dist/ManagerUI dist/ManagerCapture
