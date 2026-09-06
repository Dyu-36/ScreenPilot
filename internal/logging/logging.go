package logging

import (
	"fmt"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"time"
)

const maxLogSize int64 = 2 * 1024 * 1024

var mu sync.Mutex

func dataRoot() string {
	if runtime.GOOS == "windows" {
		if v := os.Getenv("APPDATA"); v != "" {
			return v
		}
		if home := os.Getenv("USERPROFILE"); home != "" {
			return filepath.Join(home, "AppData", "Roaming")
		}
	}
	if v := os.Getenv("XDG_DATA_HOME"); filepath.IsAbs(v) {
		return v
	}
	if home, err := os.UserHomeDir(); err == nil {
		return filepath.Join(home, ".local", "share")
	}
	return os.TempDir()
}

func ManagerDir() string {
	return filepath.Join(dataRoot(), "manager")
}

func LegacyDir() string {
	return filepath.Join(dataRoot(), "tack-agent")
}

func Path() string {
	return filepath.Join(ManagerDir(), "manager.log")
}

func Append(message string) {
	mu.Lock()
	defer mu.Unlock()

	dir := ManagerDir()
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return
	}
	path := Path()
	if info, err := os.Stat(path); err == nil && info.Size() > maxLogSize {
		rotated := path + ".1"
		_ = os.Remove(rotated)
		if err := os.Rename(path, rotated); err != nil {
			_ = os.Remove(path)
		}
	}

	clean := strings.NewReplacer("\r", " ", "\n", " ").Replace(message)
	f, err := os.OpenFile(path, os.O_CREATE|os.O_WRONLY|os.O_APPEND, 0o600)
	if err != nil {
		return
	}
	defer f.Close()
	_, _ = fmt.Fprintf(f, "[%s] %s\r\n", time.Now().Format(time.RFC3339), clean)
}

func Printf(format string, args ...any) {
	Append(fmt.Sprintf(format, args...))
}
