//go:build windows

package ipc

import (
	"fmt"
	"syscall"
	"unsafe"
)

const (
	mutexName          = `Local\Manager.SingleInstance.v1`
	eventName          = `Local\Manager.HandoffEvent.v1`
	errorAlreadyExists = 183
	eventModifyState   = 0x0002
	infinite           = 0xffffffff
)

var (
	kernel32                = syscall.NewLazyDLL("kernel32.dll")
	procCreateMutex         = kernel32.NewProc("CreateMutexW")
	procCreateEvent         = kernel32.NewProc("CreateEventW")
	procOpenEvent           = kernel32.NewProc("OpenEventW")
	procSetEvent            = kernel32.NewProc("SetEvent")
	procWaitForSingleObject = kernel32.NewProc("WaitForSingleObject")
	procCloseHandle         = kernel32.NewProc("CloseHandle")
	procGetLastError        = kernel32.NewProc("GetLastError")
)

type Lock struct {
	mutex uintptr
	event uintptr
}

func Acquire(onSignal func()) (*Lock, bool, error) {
	name, _ := syscall.UTF16PtrFromString(mutexName)
	handle, _, _ := procCreateMutex.Call(0, 0, uintptr(unsafe.Pointer(name)))
	if handle == 0 {
		return nil, false, lastError("CreateMutexW")
	}
	last, _, _ := procGetLastError.Call()
	if last == errorAlreadyExists {
		procCloseHandle.Call(handle)
		return nil, false, nil
	}

	eventW, _ := syscall.UTF16PtrFromString(eventName)
	event, _, _ := procCreateEvent.Call(0, 0, 0, uintptr(unsafe.Pointer(eventW)))
	if event == 0 {
		procCloseHandle.Call(handle)
		return nil, false, lastError("CreateEventW")
	}
	lock := &Lock{mutex: handle, event: event}
	go func() {
		for {
			result, _, _ := procWaitForSingleObject.Call(event, infinite)
			if result != 0 {
				return
			}
			onSignal()
		}
	}()
	return lock, true, nil
}

func Signal() bool {
	name, _ := syscall.UTF16PtrFromString(eventName)
	event, _, _ := procOpenEvent.Call(eventModifyState, 0, uintptr(unsafe.Pointer(name)))
	if event == 0 {
		return false
	}
	defer procCloseHandle.Call(event)
	ok, _, _ := procSetEvent.Call(event)
	return ok != 0
}

func (l *Lock) Close() error {
	if l == nil {
		return nil
	}
	if l.event != 0 {
		procCloseHandle.Call(l.event)
		l.event = 0
	}
	if l.mutex != 0 {
		procCloseHandle.Call(l.mutex)
		l.mutex = 0
	}
	return nil
}

func lastError(op string) error {
	code, _, _ := procGetLastError.Call()
	if code == 0 {
		return fmt.Errorf("%s failed", op)
	}
	return fmt.Errorf("%s failed: %w", op, syscall.Errno(code))
}
