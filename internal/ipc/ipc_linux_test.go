//go:build linux

package ipc

import (
	"os"
	"testing"
	"time"
)

func TestAcquireSignalAndRelease(t *testing.T) {
	t.Setenv("XDG_RUNTIME_DIR", t.TempDir())
	signaled := make(chan struct{}, 1)
	first, ok, err := Acquire(func() { signaled <- struct{}{} })
	if err != nil || !ok {
		t.Fatalf("first Acquire = ok %v, err %v", ok, err)
	}
	defer first.Close()

	second, ok, err := Acquire(nil)
	if err != nil {
		t.Fatal(err)
	}
	if ok || second != nil {
		t.Fatal("second instance unexpectedly acquired lock")
	}
	if !Signal() {
		t.Fatal("Signal failed")
	}
	select {
	case <-signaled:
	case <-time.After(2 * time.Second):
		t.Fatal("signal callback timeout")
	}

	if err := first.Close(); err != nil {
		t.Fatal(err)
	}
	third, ok, err := Acquire(nil)
	if err != nil || !ok {
		t.Fatalf("reacquire = ok %v, err %v", ok, err)
	}
	_ = third.Close()
	_ = os.RemoveAll(os.Getenv("XDG_RUNTIME_DIR"))
}
