//go:build !windows

package ui

import (
	"fmt"

	"github.com/Dyu-36/screenshot/internal/agent"
)

func Run(*agent.Shared) error {
	return fmt.Errorf("ManagerUI chỉ hỗ trợ Windows")
}

func RequestFocus() {}
