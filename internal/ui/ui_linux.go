//go:build linux

package ui

import (
	"context"
	"errors"
	"fmt"
	"os/exec"
	"strconv"
	"strings"
	"time"

	"github.com/Dyu-36/screenshot/internal/agent"
	"github.com/Dyu-36/screenshot/internal/config"
	"github.com/Dyu-36/screenshot/internal/logging"
	"github.com/Dyu-36/screenshot/internal/platform"
	"github.com/Dyu-36/screenshot/internal/worker"
)

func Run(shared *agent.Shared) error {
	if _, err := exec.LookPath("kdialog"); err != nil {
		return fmt.Errorf("KDE GUI cần kdialog (CachyOS: sudo pacman -S kdialog)")
	}
	if err := platform.EnsureDesktopIntegration(); err != nil {
		logging.Printf("desktop integration error: %v", err)
	}

	for {
		cfg := shared.ConfigSnapshot()
		active := "Chưa có bot"
		devices := 0
		if bot := cfg.ActiveBot(); bot != nil {
			active = bot.Label
			devices = len(bot.Devices)
		}
		prompt := fmt.Sprintf("Manager\n\nBot đang hoạt động: %s\nThiết bị đã pair: %d\n\nChọn chức năng:", active, devices)
		choice, canceled, err := menu(prompt,
			"add", "Thêm bot / nhập token",
			"bots", "Quản lý bot",
			"devices", "Quản lý thiết bị đã pair",
			"pair", "Tạo mã /pair cho bot đang hoạt động",
			"hide", "Ẩn và chuyển về Manager chạy nền",
		)
		if err != nil {
			return err
		}
		if canceled || choice == "hide" {
			return handoffToBackground()
		}

		switch choice {
		case "add":
			if err := addBot(shared); err != nil {
				showError(err.Error())
			}
		case "bots":
			if err := manageBots(shared); err != nil {
				showError(err.Error())
			}
		case "devices":
			if err := manageDevices(shared); err != nil {
				showError(err.Error())
			}
		case "pair":
			cfg := shared.ConfigSnapshot()
			bot := cfg.ActiveBot()
			if bot == nil {
				showInfo("Chưa có bot đang hoạt động.")
				continue
			}
			if err := pairToken(shared, bot.Token); err != nil {
				showError(err.Error())
			}
		}
	}
}

func RequestFocus() {
	if path, err := exec.LookPath("kdialog"); err == nil {
		_ = exec.Command(path, "--title", "Manager", "--passivepopup", "Manager đang mở.", "2").Start()
	}
}

func addBot(shared *agent.Shared) error {
	token, canceled, err := dialogOutput("--password", "Nhập Bot Token Zalo:")
	if err != nil || canceled {
		return err
	}
	token = strings.TrimSpace(token)
	if token == "" {
		return fmt.Errorf("token không được để trống")
	}
	return pairToken(shared, token)
}

func manageBots(shared *agent.Shared) error {
	cfg := shared.ConfigSnapshot()
	if len(cfg.Bots) == 0 {
		showInfo("Chưa có bot nào.")
		return nil
	}
	args := []string{"--menu", "Chọn bot:"}
	for i, bot := range cfg.Bots {
		state := ""
		if bot.Active {
			state = " [ĐANG CHẠY]"
		}
		args = append(args, strconv.Itoa(i), fmt.Sprintf("%s%s — %d thiết bị", bot.Label, state, len(bot.Devices)))
	}
	selected, canceled, err := dialogOutput(args...)
	if err != nil || canceled {
		return err
	}
	index, err := strconv.Atoi(strings.TrimSpace(selected))
	if err != nil || index < 0 || index >= len(cfg.Bots) {
		return fmt.Errorf("lựa chọn bot không hợp lệ")
	}
	bot := cfg.Bots[index]
	action, canceled, err := menu("Bot: "+bot.Label,
		"activate", "Kích hoạt bot này",
		"delete", "Xóa bot này",
		"back", "Quay lại",
	)
	if err != nil || canceled || action == "back" {
		return err
	}
	switch action {
	case "activate":
		if err := shared.Mutate(func(cfg *config.Config) { cfg.SetActive(bot.Token) }); err != nil {
			return err
		}
		if err := platform.InstallAutostart(); err != nil {
			logging.Printf("autostart error: %v", err)
		}
		showInfo("Đã chuyển bot đang hoạt động.")
	case "delete":
		yes, err := yesNo("Xóa " + bot.Label + "?")
		if err != nil || !yes {
			return err
		}
		if err := shared.Mutate(func(cfg *config.Config) { cfg.RemoveBot(bot.Token) }); err != nil {
			return err
		}
		if len(shared.ConfigSnapshot().Bots) == 0 {
			_ = platform.RemoveAutostart()
		}
		showInfo("Đã xóa bot.")
	}
	return nil
}

func manageDevices(shared *agent.Shared) error {
	cfg := shared.ConfigSnapshot()
	bot := cfg.ActiveBot()
	if bot == nil {
		showInfo("Chưa có bot đang hoạt động.")
		return nil
	}
	if len(bot.Devices) == 0 {
		showInfo("Bot đang hoạt động chưa có thiết bị nào được pair.")
		return nil
	}
	args := []string{"--menu", "Chọn thiết bị để xóa:"}
	for i, device := range bot.Devices {
		args = append(args, strconv.Itoa(i), device.ChatID)
	}
	selected, canceled, err := dialogOutput(args...)
	if err != nil || canceled {
		return err
	}
	index, err := strconv.Atoi(strings.TrimSpace(selected))
	if err != nil || index < 0 || index >= len(bot.Devices) {
		return fmt.Errorf("lựa chọn thiết bị không hợp lệ")
	}
	chatID := bot.Devices[index].ChatID
	yes, err := yesNo("Gỡ pair thiết bị " + chatID + "?")
	if err != nil || !yes {
		return err
	}
	if err := shared.Mutate(func(cfg *config.Config) { cfg.RemoveDevice(chatID) }); err != nil {
		return err
	}
	showInfo("Đã xóa thiết bị.")
	return nil
}

func pairToken(shared *agent.Shared, token string) error {
	code := agent.GeneratePairCode()
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()

	type result struct {
		chatID string
		err    error
	}
	resultCh := make(chan result, 1)
	go func() {
		chatID, err := shared.Pair(ctx, token, code, 60*time.Second)
		resultCh <- result{chatID: chatID, err: err}
	}()

	command := "/pair " + code
	_, canceled, err := dialogOutput("--inputbox",
		"Gửi lệnh dưới đây cho bot trong vòng 60 giây.\nSau khi gửi, bấm OK để chờ kết quả:", command)
	if err != nil {
		cancel()
		<-resultCh
		return err
	}
	if canceled {
		cancel()
		<-resultCh
		return nil
	}
	passive("Đang chờ bot nhận "+command+" …", 3)
	res := <-resultCh
	if res.err != nil {
		return res.err
	}
	if err := platform.InstallAutostart(); err != nil {
		logging.Printf("autostart error: %v", err)
	}
	showInfo("Đã ghép cặp thành công với " + res.chatID + ".")
	return nil
}

func handoffToBackground() error {
	if err := worker.StartSibling("Manager", "--background", "--wait-for-instance"); err != nil {
		return fmt.Errorf("không thể chuyển sang Manager chạy nền: %w", err)
	}
	logging.Append("GUI handoff: Linux UI exiting to background Manager")
	return nil
}

func menu(prompt string, pairs ...string) (string, bool, error) {
	args := []string{"--menu", prompt}
	args = append(args, pairs...)
	return dialogOutput(args...)
}

func dialogOutput(args ...string) (string, bool, error) {
	path, err := exec.LookPath("kdialog")
	if err != nil {
		return "", false, err
	}
	full := append([]string{"--title", "Manager"}, args...)
	cmd := exec.Command(path, full...)
	output, err := cmd.Output()
	if err == nil {
		return strings.TrimSpace(string(output)), false, nil
	}
	var exitErr *exec.ExitError
	if errors.As(err, &exitErr) && exitErr.ExitCode() == 1 {
		return "", true, nil
	}
	return "", false, fmt.Errorf("kdialog thất bại: %w", err)
}

func yesNo(message string) (bool, error) {
	path, err := exec.LookPath("kdialog")
	if err != nil {
		return false, err
	}
	cmd := exec.Command(path, "--title", "Manager", "--yesno", message)
	if err := cmd.Run(); err == nil {
		return true, nil
	} else {
		var exitErr *exec.ExitError
		if errors.As(err, &exitErr) && exitErr.ExitCode() == 1 {
			return false, nil
		}
		return false, err
	}
}

func showInfo(message string) {
	path, err := exec.LookPath("kdialog")
	if err == nil {
		_ = exec.Command(path, "--title", "Manager", "--msgbox", message).Run()
	}
}

func showError(message string) {
	path, err := exec.LookPath("kdialog")
	if err == nil {
		_ = exec.Command(path, "--title", "Manager", "--error", message).Run()
	}
}

func passive(message string, seconds int) {
	path, err := exec.LookPath("kdialog")
	if err == nil {
		_ = exec.Command(path, "--title", "Manager", "--passivepopup", message, strconv.Itoa(seconds)).Start()
	}
}
