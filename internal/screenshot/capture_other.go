//go:build !windows

package screenshot

import "fmt"

func CapturePrimary(string) error {
	return fmt.Errorf("screenshot chỉ hỗ trợ Windows")
}
