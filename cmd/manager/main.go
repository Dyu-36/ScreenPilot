package main

import (
	"context"
	"fmt"
	"os"
	"strings"
	"time"

	"github.com/Dyu-36/screenshot/internal/agent"
	"github.com/Dyu-36/screenshot/internal/config"
	"github.com/Dyu-36/screenshot/internal/ipc"
	"github.com/Dyu-36/screenshot/internal/logging"
	"github.com/Dyu-36/screenshot/internal/platform"
	"github.com/Dyu-36/screenshot/internal/worker"
)

type options struct {
	background      bool
	waitForInstance bool
	capture         string
	help            bool
}

func main() {
	opts, err := parseOptions(os.Args[1:])
	if err != nil {
		platform.ShowError("Tham số không hợp lệ:\n\n" + err.Error())
		os.Exit(2)
	}
	silent := opts.background || opts.waitForInstance
	if err := run(opts); err != nil {
		logging.Printf("fatal error: %v", err)
		if !silent {
			platform.ShowError("Manager không thể khởi động:\n\n" + err.Error())
		}
		os.Exit(1)
	}
}

func run(opts options) error {
	if opts.help {
		platform.ShowInfo("Manager\n\nManager.exe: mở giao diện\nManager.exe --background: chạy nền\nManager.exe --capture <path.png>: chụp qua worker one-shot")
		return nil
	}
	if opts.capture != "" {
		if err := worker.Capture(opts.capture); err != nil {
			return err
		}
		platform.ShowInfo("Đã chụp màn hình:\n" + opts.capture)
		return nil
	}
	if !opts.background {
		return worker.StartSibling("ManagerUI")
	}

	lock, acquired, err := acquireBackground(opts.waitForInstance)
	if err != nil {
		return err
	}
	if !acquired {
		return nil
	}
	defer lock.Close()

	cfg, err := config.Load()
	if err != nil {
		return err
	}
	if len(cfg.Bots) == 0 {
		_ = platform.RemoveAutostart()
		logging.Append("background: skipped because no configured bots")
		return nil
	}
	if err := platform.InstallAutostart(); err != nil {
		logging.Printf("autostart error: %v", err)
	}

	shared := agent.New(cfg)
	logging.Append("startup: Manager background started")
	shared.Run(context.Background())
	return nil
}

func acquireBackground(wait bool) (*ipc.Lock, bool, error) {
	for {
		lock, ok, err := ipc.Acquire(func() {
			logging.Append("GUI handoff: background releasing single-instance")
			os.Exit(0)
		})
		if err != nil {
			return nil, false, err
		}
		if ok {
			return lock, true, nil
		}
		if !wait {
			return nil, false, nil
		}
		time.Sleep(75 * time.Millisecond)
	}
}

func parseOptions(args []string) (options, error) {
	var opts options
	for i := 0; i < len(args); i++ {
		switch strings.ToLower(args[i]) {
		case "--background":
			opts.background = true
		case "--wait-for-instance":
			opts.waitForInstance = true
		case "--help", "-h", "/?":
			opts.help = true
		case "--capture":
			i++
			if i >= len(args) {
				return opts, fmt.Errorf("--capture cần đường dẫn file PNG")
			}
			opts.capture = args[i]
		default:
			return opts, fmt.Errorf("tham số không hỗ trợ: %s", args[i])
		}
	}
	return opts, nil
}
