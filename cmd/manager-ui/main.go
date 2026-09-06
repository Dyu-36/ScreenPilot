package main

import (
	"context"
	"os"
	"time"

	"github.com/Dyu-36/screenshot/internal/agent"
	"github.com/Dyu-36/screenshot/internal/config"
	"github.com/Dyu-36/screenshot/internal/ipc"
	"github.com/Dyu-36/screenshot/internal/logging"
	"github.com/Dyu-36/screenshot/internal/platform"
	"github.com/Dyu-36/screenshot/internal/ui"
)

func main() {
	if err := run(); err != nil {
		logging.Printf("fatal error: UI: %v", err)
		platform.ShowError("ManagerUI không thể khởi động:\n\n" + err.Error())
		os.Exit(1)
	}
}

func run() error {
	if err := platform.EnsureDesktopIntegration(); err != nil {
		logging.Printf("desktop integration error: %v", err)
	}
	lock, acquired, err := acquireUI()
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
	if len(cfg.Bots) > 0 {
		if err := platform.InstallAutostart(); err != nil {
			logging.Printf("autostart error: %v", err)
		}
	}

	shared := agent.New(cfg)
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	go shared.Run(ctx)

	logging.Append("startup: ManagerUI started")
	return ui.Run(shared)
}

func acquireUI() (*ipc.Lock, bool, error) {
	for i := 0; i < 30; i++ {
		lock, ok, err := ipc.Acquire(ui.RequestFocus)
		if err != nil {
			return nil, false, err
		}
		if ok {
			return lock, true, nil
		}
		ipc.Signal()
		time.Sleep(50 * time.Millisecond)
	}
	return nil, false, nil
}
