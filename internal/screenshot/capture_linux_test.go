//go:build linux

package screenshot

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/godbus/dbus/v5"
)

func TestEncodeKWinRGBX(t *testing.T) {
	path := filepath.Join(t.TempDir(), "shot.png")
	metadata := map[string]dbus.Variant{
		"width":  dbus.MakeVariant(uint32(2)),
		"height": dbus.MakeVariant(uint32(1)),
		"stride": dbus.MakeVariant(uint32(8)),
		"format": dbus.MakeVariant(uint32(16)),
	}
	raw := []byte{255, 0, 0, 0, 0, 255, 0, 0}
	if err := encodeKWinRaw(path, metadata, raw); err != nil {
		t.Fatal(err)
	}
	if info, err := os.Stat(path); err != nil || info.Size() == 0 {
		t.Fatalf("png missing/empty: %v", err)
	}
}

func TestStripANSI(t *testing.T) {
	got := stripANSI("Output: 1 DP-1\n\x1b[32mpriority 1\x1b[0m\n")
	if got != "Output: 1 DP-1\npriority 1\n" {
		t.Fatalf("got %q", got)
	}
}
