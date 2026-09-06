//go:build linux

package platform

import (
	"bytes"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
)

func InstallAutostart() error {
	manager, err := siblingExecutable("Manager")
	if err != nil {
		return err
	}
	dir, err := configHome("autostart")
	if err != nil {
		return err
	}
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return fmt.Errorf("không tạo được thư mục autostart: %w", err)
	}
	content := "[Desktop Entry]\n" +
		"Type=Application\n" +
		"Name=Manager\n" +
		"Comment=Manager Zalo background agent\n" +
		"Exec=" + desktopQuote(manager) + " --background\n" +
		"Terminal=false\n" +
		"X-GNOME-Autostart-enabled=true\n" +
		"X-KDE-autostart-after=panel\n"
	return writeIfChanged(filepath.Join(dir, "Manager.desktop"), []byte(content), 0o600)
}

func RemoveAutostart() error {
	dir, err := configHome("autostart")
	if err != nil {
		return err
	}
	err = os.Remove(filepath.Join(dir, "Manager.desktop"))
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	return err
}

func EnsureDesktopIntegration() error {
	manager, err := siblingExecutable("Manager")
	if err != nil {
		return err
	}
	capture, err := siblingExecutable("ManagerCapture")
	if err != nil {
		return err
	}
	appDir, err := dataHome("applications")
	if err != nil {
		return err
	}
	if err := os.MkdirAll(appDir, 0o700); err != nil {
		return fmt.Errorf("không tạo được application dir: %w", err)
	}

	managerDesktop := "[Desktop Entry]\n" +
		"Type=Application\n" +
		"Name=Manager\n" +
		"Comment=Lightweight Zalo-controlled manager\n" +
		"Exec=" + desktopQuote(manager) + "\n" +
		"Terminal=false\n" +
		"Categories=Utility;\n"
	captureDesktop := "[Desktop Entry]\n" +
		"Type=Application\n" +
		"Name=Manager Capture\n" +
		"NoDisplay=true\n" +
		"Exec=" + desktopQuote(capture) + "\n" +
		"Terminal=false\n" +
		"X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2\n"

	changed := false
	for path, data := range map[string][]byte{
		filepath.Join(appDir, "manager.desktop"):         []byte(managerDesktop),
		filepath.Join(appDir, "manager-capture.desktop"): []byte(captureDesktop),
	} {
		didChange, err := writeIfChangedStatus(path, data, 0o600)
		if err != nil {
			return err
		}
		changed = changed || didChange
	}
	if changed {
		refreshDesktopCache()
	}
	return nil
}

func ShowInfo(message string) {
	if graphical() {
		if path, err := exec.LookPath("kdialog"); err == nil {
			_ = exec.Command(path, "--title", "Manager", "--msgbox", message).Run()
			return
		}
	}
	fmt.Println(message)
}

func ShowError(message string) {
	if graphical() {
		if path, err := exec.LookPath("kdialog"); err == nil {
			_ = exec.Command(path, "--title", "Manager", "--error", message).Run()
			return
		}
	}
	fmt.Fprintln(os.Stderr, message)
}

func siblingExecutable(stem string) (string, error) {
	exe, err := os.Executable()
	if err != nil {
		return "", err
	}
	if resolved, err := filepath.EvalSymlinks(exe); err == nil {
		exe = resolved
	}
	path := filepath.Join(filepath.Dir(exe), stem)
	if resolved, err := filepath.EvalSymlinks(path); err == nil {
		path = resolved
	}
	return path, nil
}

func configHome(child string) (string, error) {
	if root := os.Getenv("XDG_CONFIG_HOME"); filepath.IsAbs(root) {
		return filepath.Join(root, child), nil
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(home, ".config", child), nil
}

func dataHome(child string) (string, error) {
	if root := os.Getenv("XDG_DATA_HOME"); filepath.IsAbs(root) {
		return filepath.Join(root, child), nil
	}
	home, err := os.UserHomeDir()
	if err != nil {
		return "", err
	}
	return filepath.Join(home, ".local", "share", child), nil
}

func desktopQuote(value string) string {
	replacer := strings.NewReplacer(`\`, `\\`, `"`, `\"`, "$", `\$`, "`", "\\`")
	return `"` + replacer.Replace(value) + `"`
}

func writeIfChanged(path string, data []byte, mode os.FileMode) error {
	_, err := writeIfChangedStatus(path, data, mode)
	return err
}

func writeIfChangedStatus(path string, data []byte, mode os.FileMode) (bool, error) {
	if old, err := os.ReadFile(path); err == nil && bytes.Equal(old, data) {
		return false, nil
	}
	tmp := path + ".tmp"
	if err := os.WriteFile(tmp, data, mode); err != nil {
		return false, err
	}
	if err := os.Rename(tmp, path); err != nil {
		_ = os.Remove(tmp)
		return false, err
	}
	return true, nil
}

func refreshDesktopCache() {
	for _, name := range []string{"kbuildsycoca6", "kbuildsycoca5"} {
		if path, err := exec.LookPath(name); err == nil {
			_ = exec.Command(path).Run()
			return
		}
	}
}

func graphical() bool {
	return os.Getenv("WAYLAND_DISPLAY") != "" || os.Getenv("DISPLAY") != ""
}
