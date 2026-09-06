//go:build !windows && !linux

package screenshot

import "fmt"

func CapturePrimary(string) error {
	return fmt.Errorf("screenshot chưa hỗ trợ hệ điều hành này")
}
