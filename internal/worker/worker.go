package worker

import (
	"bytes"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
)

func Sibling(stem string) (string, error) {
	exe, err := os.Executable()
	if err != nil {
		return "", fmt.Errorf("không xác định được executable hiện tại: %w", err)
	}
	name := stem
	if runtime.GOOS == "windows" {
		name += ".exe"
	}
	return filepath.Join(filepath.Dir(exe), name), nil
}

func Capture(output string) error {
	exe, err := Sibling("ManagerCapture")
	if err != nil {
		return err
	}
	if info, err := os.Stat(exe); err != nil || info.IsDir() {
		return fmt.Errorf("không tìm thấy %s", exe)
	}
	cmd := exec.Command(exe, output)
	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr
	err = cmd.Run()
	if err != nil {
		detail := strings.TrimSpace(stderr.String())
		if detail == "" {
			detail = strings.TrimSpace(stdout.String())
		}
		if detail == "" {
			detail = err.Error()
		}
		return fmt.Errorf("ManagerCapture thất bại: %s", detail)
	}
	info, err := os.Stat(output)
	if err != nil {
		return fmt.Errorf("ManagerCapture báo thành công nhưng không tạo file %s: %w", output, err)
	}
	if info.Size() <= 0 {
		return fmt.Errorf("ManagerCapture tạo file rỗng: %s", output)
	}
	return nil
}

func StartSibling(stem string, args ...string) error {
	exe, err := Sibling(stem)
	if err != nil {
		return err
	}
	if _, err := os.Stat(exe); err != nil {
		return fmt.Errorf("không tìm thấy %s", exe)
	}
	cmd := exec.Command(exe, args...)
	if err := cmd.Start(); err != nil {
		return fmt.Errorf("không khởi động được %s: %w", exe, err)
	}
	return nil
}
