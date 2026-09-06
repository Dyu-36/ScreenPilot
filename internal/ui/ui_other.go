//go:build !windows && !linux

package ui

import (
	"fmt"

	"github.com/Dyu-36/screenshot/internal/agent"
)

func Run(*agent.Shared) error {
	return fmt.Errorf("ManagerUI chưa hỗ trợ hệ điều hành này")
}

func RequestFocus() {}
