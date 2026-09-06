package main

import (
	"fmt"
	"os"

	"github.com/Dyu-36/screenshot/internal/screenshot"
)

func main() {
	if len(os.Args) != 2 {
		fmt.Fprintln(os.Stderr, "ManagerCapture: thiếu đường dẫn PNG đầu ra")
		os.Exit(2)
	}
	if err := screenshot.CapturePrimary(os.Args[1]); err != nil {
		fmt.Fprintln(os.Stderr, "ManagerCapture:", err)
		os.Exit(1)
	}
}
