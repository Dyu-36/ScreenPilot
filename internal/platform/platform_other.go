//go:build !windows && !linux

package platform

import "fmt"

func InstallAutostart() error         { return nil }
func RemoveAutostart() error          { return nil }
func EnsureDesktopIntegration() error { return nil }
func ShowInfo(message string)         { fmt.Println(message) }
func ShowError(message string)        { fmt.Println(message) }
