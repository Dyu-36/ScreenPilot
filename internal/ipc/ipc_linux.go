//go:build linux

package ipc

import (
	"errors"
	"fmt"
	"net"
	"os"
	"path/filepath"
	"strconv"
	"sync"
	"syscall"
	"time"
)

type Lock struct {
	file       *os.File
	listener   net.Listener
	socketPath string
	closeOnce  sync.Once
}

func runtimeDir() (string, error) {
	if dir := os.Getenv("XDG_RUNTIME_DIR"); filepath.IsAbs(dir) {
		dir = filepath.Join(dir, "manager")
		if err := os.MkdirAll(dir, 0o700); err != nil {
			return "", err
		}
		return dir, nil
	}
	dir := filepath.Join(os.TempDir(), "manager-"+strconv.Itoa(os.Getuid()))
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return "", err
	}
	return dir, nil
}

func Acquire(onSignal func()) (*Lock, bool, error) {
	dir, err := runtimeDir()
	if err != nil {
		return nil, false, fmt.Errorf("không tạo được runtime dir: %w", err)
	}
	lockPath := filepath.Join(dir, "manager.lock")
	file, err := os.OpenFile(lockPath, os.O_CREATE|os.O_RDWR, 0o600)
	if err != nil {
		return nil, false, fmt.Errorf("không mở được single-instance lock: %w", err)
	}
	if err := syscall.Flock(int(file.Fd()), syscall.LOCK_EX|syscall.LOCK_NB); err != nil {
		_ = file.Close()
		if errors.Is(err, syscall.EWOULDBLOCK) || errors.Is(err, syscall.EAGAIN) {
			return nil, false, nil
		}
		return nil, false, fmt.Errorf("không khóa được single-instance: %w", err)
	}

	socketPath := filepath.Join(dir, "manager.sock")
	_ = os.Remove(socketPath)
	listener, err := net.Listen("unix", socketPath)
	if err != nil {
		_ = syscall.Flock(int(file.Fd()), syscall.LOCK_UN)
		_ = file.Close()
		return nil, false, fmt.Errorf("không tạo được IPC socket: %w", err)
	}
	_ = os.Chmod(socketPath, 0o600)
	lock := &Lock{file: file, listener: listener, socketPath: socketPath}
	go lock.serve(onSignal)
	return lock, true, nil
}

func (l *Lock) serve(onSignal func()) {
	for {
		conn, err := l.listener.Accept()
		if err != nil {
			return
		}
		_ = conn.SetReadDeadline(time.Now().Add(time.Second))
		buf := make([]byte, 32)
		_, _ = conn.Read(buf)
		_ = conn.Close()
		if onSignal != nil {
			onSignal()
		}
	}
}

func Signal() bool {
	dir, err := runtimeDir()
	if err != nil {
		return false
	}
	conn, err := net.DialTimeout("unix", filepath.Join(dir, "manager.sock"), 300*time.Millisecond)
	if err != nil {
		return false
	}
	defer conn.Close()
	_ = conn.SetWriteDeadline(time.Now().Add(300 * time.Millisecond))
	_, err = conn.Write([]byte("signal\n"))
	return err == nil
}

func (l *Lock) Close() error {
	if l == nil {
		return nil
	}
	var closeErr error
	l.closeOnce.Do(func() {
		if l.listener != nil {
			if err := l.listener.Close(); err != nil && !errors.Is(err, net.ErrClosed) {
				closeErr = err
			}
		}
		if l.socketPath != "" {
			_ = os.Remove(l.socketPath)
		}
		if l.file != nil {
			_ = syscall.Flock(int(l.file.Fd()), syscall.LOCK_UN)
			if err := l.file.Close(); closeErr == nil {
				closeErr = err
			}
		}
	})
	return closeErr
}
