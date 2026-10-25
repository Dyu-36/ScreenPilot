//go:build windows

package platform

import (
	"fmt"
	"os"
	"path/filepath"
	"syscall"
	"unsafe"

	"github.com/Dyu-36/screenpilot/internal/logging"
)

const (
	hkeyCurrentUser   = 0x80000001
	keySetValue       = 0x0002
	regSZ             = 1
	errorFileNotFound = 2
	mbOK              = 0x00000000
	mbIconError       = 0x00000010
	mbIconInformation = 0x00000040
	mbSetForeground   = 0x00010000
)

var (
	advapi32           = syscall.NewLazyDLL("advapi32.dll")
	user32             = syscall.NewLazyDLL("user32.dll")
	procRegCreateKeyEx = advapi32.NewProc("RegCreateKeyExW")
	procRegOpenKeyEx   = advapi32.NewProc("RegOpenKeyExW")
	procRegSetValueEx  = advapi32.NewProc("RegSetValueExW")
	procRegDeleteValue = advapi32.NewProc("RegDeleteValueW")
	procRegCloseKey    = advapi32.NewProc("RegCloseKey")
	procMessageBox     = user32.NewProc("MessageBoxW")
)

func InstallAutostart() error {
	key, err := openRunKey(true)
	if err != nil {
		return err
	}
	defer procRegCloseKey.Call(key)

	exe, err := managerExecutable()
	if err != nil {
		return err
	}
	command := fmt.Sprintf("\"%s\" --background", exe)
	if err := setStringValue(key, "Manager", command); err != nil {
		return err
	}
	_ = deleteValue(key, "TackAgent")
	logging.Append("autostart: Manager installed")
	return nil
}

func RemoveAutostart() error {
	key, err := openRunKey(false)
	if err != nil {
		if err == syscall.Errno(errorFileNotFound) {
			return nil
		}
		return err
	}
	defer procRegCloseKey.Call(key)
	_ = deleteValue(key, "TackAgent")
	if err := deleteValue(key, "Manager"); err != nil && err != syscall.Errno(errorFileNotFound) {
		return err
	}
	logging.Append("autostart: Manager removed")
	return nil
}

func EnsureDesktopIntegration() error { return nil }

func managerExecutable() (string, error) {
	exe, err := os.Executable()
	if err != nil {
		return "", err
	}
	return filepath.Join(filepath.Dir(exe), "Manager.exe"), nil
}

func openRunKey(create bool) (uintptr, error) {
	path, _ := syscall.UTF16PtrFromString(`Software\Microsoft\Windows\CurrentVersion\Run`)
	var key uintptr
	if create {
		var disposition uint32
		r, _, _ := procRegCreateKeyEx.Call(
			hkeyCurrentUser,
			uintptr(unsafe.Pointer(path)),
			0, 0, 0,
			keySetValue,
			0,
			uintptr(unsafe.Pointer(&key)),
			uintptr(unsafe.Pointer(&disposition)),
		)
		if r != 0 {
			return 0, syscall.Errno(r)
		}
		return key, nil
	}
	r, _, _ := procRegOpenKeyEx.Call(
		hkeyCurrentUser,
		uintptr(unsafe.Pointer(path)),
		0,
		keySetValue,
		uintptr(unsafe.Pointer(&key)),
	)
	if r != 0 {
		return 0, syscall.Errno(r)
	}
	return key, nil
}

func setStringValue(key uintptr, name, value string) error {
	nameW, _ := syscall.UTF16PtrFromString(name)
	valueW, _ := syscall.UTF16FromString(value)
	r, _, _ := procRegSetValueEx.Call(
		key,
		uintptr(unsafe.Pointer(nameW)),
		0,
		regSZ,
		uintptr(unsafe.Pointer(&valueW[0])),
		uintptr(len(valueW)*2),
	)
	if r != 0 {
		return syscall.Errno(r)
	}
	return nil
}

func deleteValue(key uintptr, name string) error {
	nameW, _ := syscall.UTF16PtrFromString(name)
	r, _, _ := procRegDeleteValue.Call(key, uintptr(unsafe.Pointer(nameW)))
	if r != 0 {
		return syscall.Errno(r)
	}
	return nil
}

func ShowInfo(message string) {
	messageBox(message, mbOK|mbIconInformation|mbSetForeground)
}

func ShowError(message string) {
	messageBox(message, mbOK|mbIconError|mbSetForeground)
}

func messageBox(message string, flags uintptr) {
	msg, _ := syscall.UTF16PtrFromString(message)
	title, _ := syscall.UTF16PtrFromString("Manager")
	procMessageBox.Call(0, uintptr(unsafe.Pointer(msg)), uintptr(unsafe.Pointer(title)), flags)
}
